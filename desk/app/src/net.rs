//! The app's end of desk/PROTOCOL.md, on a thread of its own so that a slow or absent server never holds up a
//! frame. It keeps the account's token, gets one the first time (by the browser, or as a new guest), asks for
//! the account's state now and then, and sends the tracker's reports, oldest first, keeping in a file those the
//! server has not had yet.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde_json::Value;
mod outbox;

/// How often the account's state is asked for anew, in seconds: the lists and the desk are changed on the web page.
const STATE_EVERY: f32 = 30.0;
/// How long after a report that named unsorted programs the lists are asked for again, in seconds.
const SORTED_AFTER: f32 = 8.0;
/// How long before trying again when the server cannot be reached, in seconds.
const RETRY_AFTER: f32 = 30.0;
const VERSION: &str = "0.1.0-alpha.7";

/// What the app asks of the thread.
pub enum Ask {
    /// Live desktop diagnostics. Never queued as activity or used to earn XP.
    Status(Value),
    /// A report to send, as the tracker made it.
    Report(String, String),
    /// Save the last partial report, then clear this desktop session. True opens account selection.
    EndSession(bool, Option<(String, String)>),
    /// A page in a browser, by its title, to be found productive, neutral or distracting.
    Page(String),
    /// A ball thrown at the encounter with this id, and which kind of ball: `ball`, `great` or `ultra`.
    Throw(String, String),
    /// A change asked of the server from the panel: the path and the body. The state is asked for anew after it.
    Post(String, Value),
    /// A new account, with this starter if one was picked and this name for it if one was given; or one chosen
    /// in the browser.
    Link(Option<String>, String),
    /// The account's state, now: something has changed that only it tells.
    Refresh,
    /// The friends, their day and the trades: just asked for (no body), or changed first (a path and a body).
    Social(String, Option<Value>),
    /// A friend's whole collection, by the friend's account id: to pick from for a trade.
    FriendMons(String),
    /// A table of the hiscores, by its path: `/hiscores/mine`, `/hiscores`, `/hiscores?species=<id>`.
    Ranks(String),
    Quit,
}

/// What the thread tells the app.
pub enum Told {
    /// Whether a real request reached the server successfully.
    Connection(bool),
    /// The answer of `GET /state`.
    State(Value),
    /// The answer of a `POST /report`.
    Reported(Value),
    /// The answer of a `POST /throw` at the encounter named, or the code of why not (`away` with no server).
    Threw(String, Result<Value, String>),
    /// What a page was found to be: its title, the list's name (none if nobody could say), and whether that is
    /// what its whole site is.
    Page(String, Option<String>, bool, f32),
    /// There is no account yet: the app asks which way to get one (`Ask::Guest` or `Ask::Link`).
    NoAccount,
    SessionEnded(Result<bool, String>),
    /// The answer of `GET /friends`, or of anything that changes what it says.
    Social(Value),
    /// A friend's collection: their account id, and the answer of `GET /friends/mons`.
    FriendMons(String, Value),
    /// A table of the hiscores: the path asked, and its answer.
    Ranks(String, Value),
    /// Something for a person to read.
    Note(String),
}

pub struct Link {
    pub server: String,
    pub home: PathBuf,
    /// Make a new guest account when there is no token, rather than ask in the browser.
    pub guest: bool,
    /// Wakes the app's own thread to hear what it has been told.
    pub wake: Box<dyn Fn() + Send>,
}

enum How {
    Guest(Option<String>, String),
    Link(Option<String>, String),
    Switch,
}

enum Failed {
    /// The server could not be reached, or did not answer as itself.
    Away(String),
    /// It answered that the token is not good, or the link is over.
    Refused(u16, String),
}

struct Net {
    link: Link,
    agent: ureq::Agent,
    told: Sender<Told>,
    token: Option<String>,
    account: Option<String>,
    unsaved: Vec<(String,String)>,
    /// How an account is to be got, once the app has said: a new guest with this starter, or by the browser.
    how: Option<How>,
    /// Whether the app has been told there is no account, so that it is told once.
    asked: bool,
    /// A link begun in the browser and not yet let in: its code and secret, and how often to ask.
    linking: Option<(String, String, f32)>,
    state_at: Option<Instant>,
    /// When the lists are to be asked for again because a report named programs the server may by then have sorted.
    recheck: Option<Instant>,
    /// What was last said of the server being away, so that it is said once.
    away: bool,
}

impl Net {
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Failed> {
        let mut request = self.agent.request(method, &format!("{}{path}", self.link.server));
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let answer = match body {
            Some(body) => request.send_json(body.clone()),
            None => request.call(),
        };
        match answer {
            Ok(answer) => {
                let value = answer.into_json().map_err(|e| Failed::Away(e.to_string()))?;
                self.tell(Told::Connection(true));
                Ok(value)
            },
            Err(ureq::Error::Status(status, answer)) => {
                let said: Value = answer.into_json().unwrap_or(Value::Null);
                Err(Failed::Refused(status, said["code"].as_str().unwrap_or("").to_string()))
            }
            Err(error) => Err(Failed::Away(error.to_string())),
        }
    }

    fn tell(&self, told: Told) {
        let _ = self.told.send(told);
        (self.link.wake)();
    }

    fn note(&self, text: impl Into<String>) {
        self.tell(Told::Note(text.into()));
    }

    fn keep_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
        if let Err(error) = std::fs::write(self.link.home.join("token"), token) {
            self.note(format!("the token could not be kept: {error}"));
        }
    }

    /// One step toward having a token. How long to wait before the next, in seconds.
    fn sign_in(&mut self) -> Result<f32, Failed> {
        let starter = match &self.how {
            // Not said yet: the app is asked, once, and nothing more is done until it answers.
            None => {
                if !self.asked {
                    self.asked = true;
                    self.tell(Told::NoAccount);
                }
                return Ok(3600.0);
            }
            Some(How::Guest(starter, name)) => Some((starter.clone(), name.clone())),
            Some(How::Link(..)) | Some(How::Switch) => None,
        };
        if let Some((starter, name)) = starter {
            let mut body = starter.map_or(serde_json::json!({}), |starter| serde_json::json!({ "starter": starter }));
            if !name.trim().is_empty() {
                body["name"] = name.trim().into();
            }
            let made = self.call("POST", "/guest", Some(&body))?;
            self.keep_token(made["token"].as_str().unwrap_or_default());
            self.note(format!("a new account: {}", made["user"]["name"].as_str().unwrap_or("?")));
            self.tell(Told::State(made["state"].clone()));
            self.state_at = Some(Instant::now());
            return Ok(0.0);
        }
        match self.linking.clone() {
            None => {
                let (starter, name) = match &self.how { Some(How::Link(starter, name)) => (starter.clone(), name.clone()), _ => (None, String::new()) };
                let legacy = std::fs::read_to_string(self.link.home.join("legacy-token")).ok();
                let switching = matches!(self.how,Some(How::Switch));
                let begun = self.call("POST", "/link/start", Some(&serde_json::json!({"starter":starter,"name":name,"legacyToken":if switching {None} else {legacy.as_deref().map(str::trim)},"switchAccount":switching})))?;
                let (code, secret, url) = (begun["code"].as_str().unwrap_or_default(), begun["secret"].as_str().unwrap_or_default(), begun["url"].as_str().unwrap_or_default());
                let every = begun["pollEvery"].as_f64().unwrap_or(2.0) as f32;
                self.linking = Some((code.to_string(), secret.to_string(), every));
                self.note(format!("Finish sign-in in your browser, then confirm connection code {code}."));
                crate::desk::open(url);
                Ok(every)
            }
            Some((code, secret, every)) => match self.call("GET", &format!("/link/poll?code={code}&secret={secret}"), None) {
                Ok(answer) if answer["status"] == "ok" => {
                    self.linking = None;
                    self.keep_token(answer["token"].as_str().unwrap_or_default());
                    self.note(format!("Connected as {}", answer["user"]["name"].as_str().unwrap_or("?")));
                    Ok(0.0)
                }
                Ok(_) => Ok(every),
                // The code ran out: the next step begins another, a while from now, not at once in a loop.
                Err(Failed::Refused(404, _)) => {
                    self.linking = None;
                    Ok(RETRY_AFTER)
                }
                Err(other) => Err(other),
            },
        }
    }

    fn enqueue(&mut self, account: String, report: String) {
        self.unsaved.push((account,report));
        if let Err(error)=self.persist_reports() {self.note(format!("Pending activity could not be saved yet: {error}"));}
    }

    fn persist_reports(&mut self) -> std::io::Result<()> {
        while let Some((account,report))=self.unsaved.first() {
            outbox::append(&self.link.home,account,report)?;
            self.unsaved.remove(0);
        }
        Ok(())
    }

    fn end_session(&mut self, switch: bool, report: Option<(String,String)>, device: uuid::Uuid) -> Result<bool,String> {
        if let Some((account,report))=report {self.enqueue(account,report);}
        self.persist_reports().map_err(|error|format!("Could not save pending activity. You are still signed in: {error}"))?;
        outbox::sign_out(&self.link.home).map_err(|error|format!("Could not finish signing out: {error}"))?;
        if let Some(token)=&self.token {
            // Best effort: failure to contact the server never prevents a local sign-out.
            let _=self.agent.post(&format!("{}/desktop/status",self.link.server)).timeout(Duration::from_secs(2))
                .set("Authorization",&format!("Bearer {token}")).send_json(serde_json::json!({"device":device.to_string(),"signedOut":true,"version":VERSION}));
        }
        self.token=None;self.account=None;self.state_at=None;self.recheck=None;self.linking=None;
        self.how=if switch {Some(How::Switch)} else {None};self.asked=true;self.away=false;
        Ok(switch)
    }

    /// Sends the reports the server has not had, oldest first, and keeps those it still has not.
    fn send_reports(&mut self) -> Result<(), Failed> {
        let Some(account)=&self.account else {return Ok(())};
        let paths=outbox::batches(&self.link.home,account).map_err(|error|Failed::Away(error.to_string()))?;
        for path in paths {
        let text = std::fs::read_to_string(&path).map_err(|error|Failed::Away(error.to_string()))?;
        let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
        let mut sent = 0;
        let mut failed = None;
        for line in &lines {
            let Ok(report) = serde_json::from_str::<Value>(line) else {
                sent += 1;
                continue;
            };
            match self.call("POST", "/report", Some(&report)) {
                Ok(answer) => {
                    // It named programs in no list: a server that sorts them by itself will have, a few seconds on,
                    // and the lists are then asked for again rather than in five minutes.
                    if report["unknown"].as_array().is_some_and(|unknown| !unknown.is_empty()) && self.recheck.is_none() {
                        self.recheck = Some(Instant::now() + Duration::from_secs_f32(SORTED_AFTER));
                    }
                    self.tell(Told::Reported(answer));
                }
                // Not a report the server will ever take: nothing is gained by keeping it.
                Err(Failed::Refused(400, _)) => {}
                Err(other) => {
                    failed = Some(other);
                    break;
                }
            }
            sent += 1;
        }
        if sent > 0 {
            let left: String = lines[sent..].iter().map(|line| format!("{line}\n")).collect();
            if let Err(error) = std::fs::write(&path, left) {
                return Err(Failed::Away(format!("Pending activity could not be updated: {error}")));
            }
        }
        if let Some(error)=failed {return Err(error);}
        }
        Ok(())
    }

    /// Everything there is to do now. How long before there may be more, in seconds.
    fn step(&mut self) -> Result<f32, Failed> {
        self.persist_reports().map_err(|error|Failed::Away(format!("Pending activity could not be saved: {error}")))?;
        if self.token.is_none() {
            let wait = self.sign_in()?;
            if self.token.is_none() {
                return Ok(wait);
            }
        }
        if self.recheck.is_some_and(|at| at <= Instant::now()) {
            (self.recheck, self.state_at) = (None, None);
        }
        if self.state_at.is_none_or(|at| at.elapsed().as_secs_f32() >= STATE_EVERY) {
            let state = self.call("GET", "/state", None)?;
            if let Some(id) = state["account"]["id"].as_str() {
                outbox::select(&self.link.home, id).map_err(|error| Failed::Away(format!("Pending reports could not be kept safely: {error}")))?;
                self.account=Some(id.to_string());
            }
            self.state_at = Some(Instant::now());
            self.tell(Told::State(state));
        }
        self.send_reports()?;
        let until_state = self.state_at.map_or(0.0, |at| STATE_EVERY - at.elapsed().as_secs_f32());
        let until_recheck = self.recheck.map_or(f32::MAX, |at| at.saturating_duration_since(Instant::now()).as_secs_f32());
        Ok(until_state.min(until_recheck).max(0.2))
    }
}

/// Reports collected for a different (or unknown legacy) account stay on this PC instead of crossing accounts.
/// Runs until asked to quit. Reports asked for while the server is away wait in the file.
pub fn run(link: Link, asks: Receiver<Ask>, told: Sender<Told>) {
    let device_file = link.home.join("device-id");
    let device = std::fs::read_to_string(&device_file).ok().and_then(|text|uuid::Uuid::parse_str(text.trim()).ok()).unwrap_or_else(uuid::Uuid::new_v4);
    let _ = std::fs::write(&device_file, device.to_string());
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    let token = std::fs::read_to_string(link.home.join("token")).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let how = link.guest.then_some(How::Guest(None, String::new()));
    let mut net = Net { link, agent, told, token, account:None, unsaved:Vec::new(), how, asked: false, linking: None, state_at: None, recheck: None, away: false };
    loop {
        let wait = match net.step() {
            Ok(wait) => {
                net.away = false;
                wait
            }
            Err(Failed::Refused(403, code)) if code == "sign-in-required" => {
                // Keep the old credential so signing in can attach the existing guest's progress.
                let _ = std::fs::copy(net.link.home.join("token"), net.link.home.join("legacy-token"));
                net.token = None;
                net.state_at = None;
                net.how = None;
                net.asked = false;
                let _ = std::fs::remove_file(net.link.home.join("token"));
                net.note("Sign in with Google, Discord, or email to keep playing.");
                0.0
            }
            Err(Failed::Refused(401, code)) => {
                // The account is gone or the token is another server's: begin again.
                net.note(format!("the server does not know this app any more ({code}): signing in again"));
                net.token = None;
                net.state_at = None;
                net.asked = false;
                if !net.link.guest {
                    net.how = None;
                }
                let _ = std::fs::remove_file(net.link.home.join("token"));
                RETRY_AFTER
            }
            Err(Failed::Refused(status, code)) => {
                net.note(format!("the server said {status} {code}"));
                RETRY_AFTER
            }
            Err(Failed::Away(why)) => {
                net.tell(Told::Connection(false));
                if !net.away {
                    net.away = true;
                    net.note(format!("{} cannot be reached ({why}): reports are kept until it can", net.link.server));
                }
                RETRY_AFTER
            }
        };
        match asks.recv_timeout(Duration::from_secs_f32(wait.max(0.05))) {
            Ok(ref ask) if net.token.is_none() && !matches!(ask,Ask::Link(..)|Ask::EndSession(..)|Ask::Report(..)|Ask::Refresh|Ask::Quit) => {},
            Ok(Ask::EndSession(switch,report)) => {
                let result=net.end_session(switch,report,device);
                net.tell(Told::SessionEnded(result));
            }
            Ok(Ask::Status(mut status)) => {
                if net.token.is_some() {
                    status["device"] = serde_json::json!(device.to_string());
                    status["version"] = serde_json::json!(VERSION);
                    match net.call("POST", "/desktop/status", Some(&status)) {
                        Ok(_) => {},
                        Err(Failed::Away(_)) => net.tell(Told::Connection(false)),
                        Err(Failed::Refused(_, _)) => {},
                    }
                }
            }
            Ok(Ask::Report(account,report)) => net.enqueue(account,report),
            Ok(Ask::Page(title)) => {
                // Unanswered (no sorter, no server) it is nobody-could-say: the page counts as its browser does.
                let answer = net.call("POST", "/page", Some(&serde_json::json!({ "title": title }))).unwrap_or(Value::Null);
                net.tell(Told::Page(title, answer["class"].as_str().map(str::to_string), answer["by"] == "site", answer["likely"].as_f64().unwrap_or(1.0) as f32));
            }
            Ok(Ask::Throw(encounter, ball)) => {
                let answer = match net.call("POST", "/throw", Some(&serde_json::json!({ "encounter": encounter, "ball": ball, "day": crate::desk::local_day() }))) {
                    Ok(answer) => Ok(answer),
                    Err(Failed::Refused(_, code)) => Err(code),
                    Err(Failed::Away(_)) => Err("away".to_string()),
                };
                net.tell(Told::Threw(encounter, answer));
            }
            Ok(Ask::Post(path, mut body)) => {
                // What is bought may earn experience (a berry), which is counted in the day it is here.
                if path == "/shop/buy" || path == "/expedition/claim" {
                    body["day"] = Value::String(crate::desk::local_day());
                }
                match net.call("POST", &path, Some(&body)) {
                    // What a mon came back from its expedition with is said: nothing else shows it.
                    Ok(answer) if answer["reward"].is_object() => {
                        let reward = &answer["reward"];
                        let mut said = format!("back from its expedition with {} points", reward["points"]);
                        for (key, name) in [("balls", "balls"), ("great", "great balls"), ("ultra", "ultra balls")] {
                            if reward[key].as_i64().unwrap_or(0) > 0 {
                                said += &format!(", {} {name}", reward[key]);
                            }
                        }
                        if reward["brought"].as_bool().unwrap_or(false) {
                            said += ", and a wild mon that followed it home";
                        }
                        net.note(said);
                    }
                    // What was asked for was done: said, so that a press is seen to have done something.
                    Ok(_) => {
                        let done = match (path.as_str(), body["item"].as_str()) {
                            ("/shop/buy", Some("berry")) => "A berry, gladly eaten.",
                            ("/shop/buy", Some("lure" | "super-lure" | "max-lure")) => "Lure active. Only productive time uses it.",
                            ("/shop/buy", Some("slot")) => "Bought: the desk holds one more.",
                            ("/shop/buy", _) => "Bought.",
                            ("/session/start", _) => "The session is on. Off you go.",
                            ("/session/stop", _) => "",
                            ("/expedition/start", _) => "Off it goes. It comes back as you work.",
                            ("/expedition/recall", _) => "Called back.",
                            ("/mons/name", _) => "Named.",
                            ("/mons/desk", _) => "The desk is changed.",
                            _ => "",
                        };
                        if !done.is_empty() {
                            net.note(done);
                        }
                    }
                    Err(Failed::Refused(_, code)) => net.note(match code.as_str() {
                        "in-session" => "a focus session is already going".to_string(),
                        "away" => "that mon is away on an expedition".to_string(),
                        "last-mon" => "one mon has to stay with you".to_string(),
                        "not-ready" => "it is not back yet".to_string(),
                        "too-many" => "that is as many as there can be at once".to_string(),
                        "no-points" => "not enough points".to_string(),
                        other => format!("the server would not: {other}"),
                    }),
                    Err(Failed::Away(_)) => net.note("the server cannot be reached: nothing was changed"),
                }
                // Whatever it did, what the account now has is asked for.
                net.state_at = None;
            }
            Ok(Ask::Link(starter, name)) => {
                if !matches!(net.how,Some(How::Switch)) {net.how = Some(How::Link(starter, name));}
                net.linking = None;
            }
            Ok(Ask::Refresh) => net.state_at = None,
            Ok(Ask::Social(path, body)) => {
                // The day is the one it is here, as in the reports: the board is of a day.
                let day = crate::desk::local_day();
                let answer = match body {
                    None => net.call("GET", &format!("/friends?day={day}"), None),
                    Some(mut body) => {
                        body["day"] = Value::String(day);
                        // A trade taken changes who has which mon, which only the state tells.
                        net.state_at = None;
                        net.call("POST", &path, Some(&body))
                    }
                };
                let done = match path.as_str() {
                    "/friends/add" => "Asked. They will see it in their Social tab.",
                    "/friends/accept" => "You are friends.",
                    "/friends/remove" => "Done.",
                    "/trades/offer" => "Offered. Nothing moves until they say yes.",
                    "/trades/accept" => "Traded!",
                    "/trades/cancel" => "The offer is off.",
                    _ => "",
                };
                match answer {
                    Ok(answer) => {
                        net.tell(Told::Social(answer));
                        if !done.is_empty() {
                            net.note(done);
                        }
                    }
                    Err(Failed::Refused(_, code)) => net.note(match code.as_str() {
                        "no-code" => "nobody has that friend code".to_string(),
                        "too-many" => "that is as many as there can be at once".to_string(),
                        "not-friends" => "you are not friends with them (any more)".to_string(),
                        "no-mon" => "one of those mons is not there to trade any more".to_string(),
                        "trade-gone" => "that trade fell through: one of the mons had gone".to_string(),
                        "no-trade" | "no-request" => "that was no longer open".to_string(),
                        other => format!("the server would not: {other}"),
                    }),
                    Err(Failed::Away(_)) => net.note("the server cannot be reached: nothing was changed"),
                }
            }
            Ok(Ask::Ranks(path)) => {
                if let Ok(answer) = net.call("GET", &path, None) {
                    net.tell(Told::Ranks(path, answer));
                }
            }
            Ok(Ask::FriendMons(id)) => {
                if let Ok(answer) = net.call("GET", &format!("/friends/mons?id={id}"), None) {
                    net.tell(Told::FriendMons(id, answer));
                }
            }
            Ok(Ask::Quit) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use std::io::{BufRead,Read,Write};
    fn client(server: String) -> Net {
        let home=std::env::temp_dir().join(format!("productivitymon-session-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&home).unwrap();let(told,_)=std::sync::mpsc::channel();
        Net{link:Link{server,home,guest:false,wake:Box::new(||{})},agent:ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build(),told,token:Some("fixture-token".into()),account:None,unsaved:Vec::new(),how:None,asked:false,linking:None,state_at:None,recheck:None,away:false}
    }
    #[test]
    fn the_network_sends_only_the_current_accounts_reports_and_restores_the_other_on_return() {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
        let worker=std::thread::spawn(move||{
            let mut bodies=Vec::new();
            for _ in 0..2 {
                let(stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut reader=std::io::BufReader::new(stream);let mut length=0;
                loop {let mut line=String::new();reader.read_line(&mut line).unwrap();if line=="\r\n"{break}if line.to_lowercase().starts_with("content-length:"){length=line.split(':').nth(1).unwrap().trim().parse::<usize>().unwrap();}}
                let mut body=vec![0;length];reader.read_exact(&mut body).unwrap();bodies.push(serde_json::from_slice::<Value>(&body).unwrap());
                let response=b"{}";write!(reader.get_mut(),"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len()).unwrap();reader.get_mut().write_all(response).unwrap();
            }
            bodies
        });
        let mut net=client(format!("http://{address}"));let a=uuid::Uuid::new_v4().to_string();let b=uuid::Uuid::new_v4().to_string();
        net.enqueue(a.clone(),r#"{"owner":"a"}"#.into());net.enqueue(b.clone(),r#"{"owner":"b"}"#.into());
        net.account=Some(b);assert!(net.send_reports().is_ok());
        assert!(std::fs::read_to_string(&outbox::batches(&net.link.home,&a).unwrap()[0]).unwrap().contains("\"a\""));
        net.account=Some(a);assert!(net.send_reports().is_ok());
        assert_eq!(worker.join().unwrap(),vec![serde_json::json!({"owner":"b"}),serde_json::json!({"owner":"a"})]);
    }
    #[test]
    fn local_sign_out_works_offline_and_keeps_the_final_partial_report_with_its_owner() {
        let mut net=client("http://127.0.0.1:9".into());let account=uuid::Uuid::new_v4().to_string();
        outbox::select(&net.link.home,&account).unwrap();std::fs::write(net.link.home.join("token"),"fixture-token").unwrap();net.account=Some(account.clone());
        assert_eq!(net.end_session(false,Some((account.clone(),"last partial report".into())),uuid::Uuid::new_v4()).unwrap(),false);
        assert!(net.token.is_none() && net.account.is_none() && net.how.is_none());
        assert!(!net.link.home.join("token").exists());
        assert_eq!(std::fs::read_to_string(&outbox::batches(&net.link.home,&account).unwrap()[0]).unwrap(),"last partial report\n");
    }
    #[test]
    fn switch_opens_account_selection_and_failed_saving_keeps_the_current_session() {
        let mut net=client("http://127.0.0.1:9".into());
        assert!(net.end_session(true,None,uuid::Uuid::new_v4()).unwrap());assert!(matches!(net.how,Some(How::Switch)));
        net.token=Some("still-signed-in".into());std::fs::write(net.link.home.join("token"),"still-signed-in").unwrap();
        std::fs::write(net.link.home.join("pending-reports"),"not a directory").unwrap();
        let result=net.end_session(false,Some((uuid::Uuid::new_v4().to_string(),"unsaved time".into())),uuid::Uuid::new_v4());
        assert!(result.is_err());assert!(net.token.is_some());assert!(net.link.home.join("token").exists());assert_eq!(net.unsaved.len(),1);
    }
}
