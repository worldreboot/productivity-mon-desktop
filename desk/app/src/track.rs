//! What the app knows of how the time at this PC is spent: which program is in front, whether anyone is there,
//! and so how many seconds went to productive, neutral and distracting programs since the last report. It is
//! told the front program and the time; it asks Windows nothing and reads no keys. Of window titles it is told
//! one kind only: a browser's, which is the page in front, so that a page can count for what it is rather than
//! for what a browser is.
//!
//! The report is desk/PROTOCOL.md's `POST /report`.

use std::collections::{BTreeSet, HashMap};

/// With no key or mouse touched for this long, nobody is counted as there, in seconds.
pub const AWAY_AFTER: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    Productive,
    Neutral,
    Distracting,
}

impl Class {
    pub fn named(name: &str) -> Option<Class> {
        match name {
            "productive" => Some(Class::Productive),
            "neutral" => Some(Class::Neutral),
            "distracting" => Some(Class::Distracting),
            _ => None,
        }
    }
}

/// A program's file name alone, in lower case, as the server keeps it: `C:\Apps\Code.exe` is `code.exe`.
pub fn program(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_lowercase()
}

/// The browsers whose window title is the page they show.
const BROWSERS: [&str; 11] = ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "arc.exe", "zen.exe", "librewolf.exe", "chromium.exe", "safari"];
/// What a browser puts after the page's own title, by which the two are told apart.
const BROWSER_NAMES: [&str; 10] = ["Google Chrome", "Microsoft Edge", "Mozilla Firefox", "Firefox", "Brave", "Opera", "Vivaldi", "Arc", "Chromium", "Safari"];
/// The most pages remembered: past that they are all forgotten and learnt again as they come up.
const PAGES_MOST: usize = 1000;

/// The page a browser's window title names, without the browser's own name after it: none for a program that is
/// not a browser, or a window that names no page (a new tab).
pub fn page(exe: &str, title: &str) -> Option<String> {
    if !BROWSERS.contains(&exe) {
        return None;
    }
    // Edge puts characters that cannot be seen in its name.
    let title: String = title.chars().filter(|c| !matches!(c, '\u{200b}' | '\u{200e}' | '\u{feff}')).collect();
    let mut parts: Vec<&str> = title.split(" - ").flat_map(|part| part.split(" \u{2014} ")).flat_map(|part| part.split(" \u{2013} ")).map(str::trim).collect();
    while parts.last().is_some_and(|last| BROWSER_NAMES.iter().any(|name| last.contains(name))) {
        parts.pop();
    }
    let page = parts.join(" - ");
    let page: String = page.chars().take(200).collect();
    (!page.is_empty() && !matches!(page.as_str(), "New Tab" | "New tab" | "Start" | "Untitled")).then_some(page)
}

/// The site a page is of, as its title names it: the last part of a title such as "std::rc - Rust" or
/// "Pull request #4 \u{b7} acme/api \u{b7} GitHub", in lower case. None for a title in one part, or whose last part
/// does not read as a name (too long, an address, a sentence). The app and the server each have this, the same.
pub fn site_of(title: &str) -> Option<String> {
    let parts: Vec<&str> = title
        .split(" - ")
        .flat_map(|part| part.split(" | "))
        .flat_map(|part| part.split(" \u{b7} "))
        .flat_map(|part| part.split(" \u{2014} "))
        .flat_map(|part| part.split(" \u{2013} "))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let last = *parts.last()?;
    let named = parts.len() >= 2 && (2..=30).contains(&last.chars().count()) && last.split_whitespace().count() <= 3 && !last.contains(['@', '/', '\\', ':']);
    named.then(|| last.to_lowercase())
}

#[derive(Default)]
pub struct Tracker {
    rules: HashMap<String, Class>,
    /// The program in front, if any is, and the page it shows if it is a browser showing one.
    front: Option<String>,
    page: Option<String>,
    /// Last external app/site classification for local correction controls. Never timed or reported.
    previous: Option<(String,Option<String>,Option<Class>)>,
    /// What pages have been found to be, by their titles: none for one nobody could say.
    pages: HashMap<String, Option<Class>>,
    /// The sites met, by name: what the server says a site is as a whole, where it does.
    sites: std::collections::BTreeMap<String, Option<Class>>,
    /// Seconds since the last report in productive, neutral and distracting programs.
    seconds: [f64; 3],
    /// Of the productive seconds, those in which the keyboard or mouse was touched; and the productive seconds by
    /// the program they were in.
    active: f64,
    by_program: HashMap<String, f64>,
    /// How sure the server's sorter was of what it said: of each program's and site's rule (`site:<name>`), of
    /// each page, and of each site taken as a whole. Where nothing is said it was wholly sure (or was not the
    /// sorter at all).
    sure: HashMap<String, f32>,
    page_sure: HashMap<String, f32>,
    /// The productive seconds since the last report, each counted by how sure the sorter was of it.
    sure_seconds: f64,
    /// Programs seen in front since the last report that are in no list.
    unknown: BTreeSet<String>,
}

impl Tracker {
    /// File name only; never a browser title. Used for private-account presence.
    pub fn program_name(&self) -> Option<&str> { self.front.as_deref() }
    /// The lists, as `GET /state` gives them: a program's name to its class.
    pub fn set_rules(&mut self, rules: &serde_json::Value) {
        self.rules = rules.as_object().into_iter().flatten().filter_map(|(exe, class)| Some((program(exe), Class::named(class.as_str()?)?))).collect();
    }

    /// How sure the sorter was of each rule, as `GET /state` gives it: a program's name to 0..1.
    pub fn set_sure(&mut self, sure: &serde_json::Value) {
        self.sure = sure.as_object().into_iter().flatten().filter_map(|(exe, likely)| Some((program(exe), likely.as_f64()? as f32))).collect();
    }

    /// How sure the sorter was of what is in front being what it counts as, 0 to 1: by the same order `judged`
    /// goes in, then the program's own rule.
    fn sureness(&self) -> f32 {
        let known = |key: &str| self.sure.get(key).copied().unwrap_or(1.0);
        if let Some(page) = &self.page {
            if let Some(site) = site_of(page) {
                if self.rules.contains_key(&format!("site:{site}")) {
                    return known(&format!("site:{site}"));
                }
                if self.sites.get(&site).is_some_and(|whole| whole.is_some()) {
                    return self.page_sure.get(&format!("site:{site}")).copied().unwrap_or(1.0);
                }
            }
            if self.pages.get(page).is_some_and(|class| class.is_some()) {
                return self.page_sure.get(page).copied().unwrap_or(1.0);
            }
        }
        self.front.as_deref().map_or(1.0, known)
    }

    /// Another program has come to the front, or none has.
    pub fn front(&mut self, path: Option<&str>) {
        self.front_page(path, None);
    }

    /// Another program has come to the front, or its window's title has changed. For a browser on a page not met
    /// before, of no site that is already known for what it is, the page's title: what it is has to be asked
    /// (`learn` is told the answer).
    pub fn front_page(&mut self, path: Option<&str>, title: Option<&str>) -> Option<String> {
        if let Some(front)=&self.front {
            self.previous=Some((front.clone(),self.page.as_deref().and_then(site_of),self.judged().or_else(||self.rules.get(front).copied())));
        }
        self.front = path.map(program);
        self.page = self.front.as_deref().zip(title).and_then(|(exe, title)| page(exe, title));
        let page = self.page.clone()?;
        if let Some(site) = site_of(&page) {
            let known = self.rules.contains_key(&format!("site:{site}")) || self.sites.get(&site).is_some_and(|whole| whole.is_some());
            self.sites.entry(site).or_insert(None);
            if known {
                return None;
            }
        }
        if self.pages.contains_key(&page) {
            return None;
        }
        if self.pages.len() >= PAGES_MOST {
            self.pages.clear();
        }
        // Kept as one nobody could say until somebody does, so that it is asked about once.
        self.pages.insert(page.clone(), None);
        Some(page)
    }

    /// What a page was found to be, or that nobody could say; `whole_site` where that is what its whole site is.
    /// `likely` is how sure of it the sorter was, 0 to 1.
    pub fn learn(&mut self, page: &str, class: Option<Class>, whole_site: bool, likely: f32) {
        if self.page_sure.len() >= PAGES_MOST * 2 {
            self.page_sure.clear();
        }
        match site_of(page) {
            Some(site) if whole_site => {
                self.page_sure.insert(format!("site:{site}"), likely);
                self.sites.insert(site, class);
            }
            _ => {
                self.page_sure.insert(page.to_string(), likely);
                self.pages.insert(page.to_string(), class);
            }
        }
    }

    /// What the page in front counts as by itself, if anything: the user's own word on its site first, then what
    /// its site is as a whole, then what the page was found to be.
    fn judged(&self) -> Option<Class> {
        let page = self.page.as_ref()?;
        let site = site_of(page);
        site.as_ref()
            .and_then(|site| self.rules.get(&format!("site:{site}")).copied().or_else(|| self.sites.get(site).copied().flatten()))
            .or_else(|| self.pages.get(page).copied().flatten())
    }

    /// What is in front and what it counts as, for a person to read: the page or the program, and its list if it
    /// is in one (a page's own, where it has one, before its browser's).
    pub fn now(&self) -> Option<(String, Option<Class>)> {
        let front = self.front.as_ref()?;
        Some((self.page.clone().unwrap_or_else(|| front.clone()), self.judged().or_else(|| self.rules.get(front).copied())))
    }

    pub fn previous_classification(&self) -> Option<(&str,Option<&str>,Option<Class>)> {
        self.previous.as_ref().map(|(program,site,class)|(program.as_str(),site.as_deref(),*class))
    }

    /// The sites met since the app started and those the user has a word on, by name: what each counts as, and
    /// whether that is the user's own word.
    pub fn sites(&self) -> Vec<(String, Option<Class>, bool)> {
        let mut all: std::collections::BTreeMap<String, (Option<Class>, bool)> = self.sites.iter().map(|(site, whole)| (site.clone(), (*whole, false))).collect();
        for (key, class) in &self.rules {
            if let Some(site) = key.strip_prefix("site:") {
                all.insert(site.to_string(), (Some(*class), true));
            }
        }
        all.into_iter().map(|(site, (class, own))| (site, class, own)).collect()
    }

    /// `seconds` have gone by with the same program in front, the last key or mouse touch `untouched` seconds ago
    /// at the end of them.
    pub fn pass(&mut self, seconds: f64, untouched: f32) {
        let Some(front) = &self.front else { return };
        // Only the part of it before they went away, if they did.
        let there = (seconds - (untouched - AWAY_AFTER).max(0.0) as f64).clamp(0.0, seconds);
        if there <= 0.0 {
            return;
        }
        // A page that counts as something by itself counts as that, whatever its browser is.
        match self.judged().or_else(|| self.rules.get(front).copied()) {
            Some(class) => {
                self.seconds[class as usize] += there;
                if class == Class::Productive {
                    self.sure_seconds += there * self.sureness().clamp(0.0, 1.0) as f64;
                    // Touched within this stretch of time, it was worked in; not touched, it was only open.
                    if (untouched as f64) < seconds {
                        self.active += there;
                    }
                    *self.by_program.entry(front.clone()).or_default() += there;
                }
            }
            None => {
                self.seconds[Class::Neutral as usize] += there;
                self.unknown.insert(front.clone());
            }
        }
    }

    /// Whether there is anything to say.
    pub fn any(&self) -> bool {
        self.seconds.iter().any(|s| *s > 0.0) || !self.unknown.is_empty()
    }

    /// What has gone by since the last report, as the server is sent it, and a clean slate.
    pub fn report(&mut self) -> serde_json::Value {
        let round = |seconds: f64| (seconds * 10.0).round() / 10.0;
        // How much of the productive time was worked in, and the program most of it was in.
        let active = if self.seconds[0] > 0.0 { ((self.active / self.seconds[0]).clamp(0.0, 1.0) * 100.0).round() / 100.0 } else { 1.0 };
        let most = self.by_program.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(program, _)| program.clone());
        let report = serde_json::json!({
            "seconds": { "productive": round(self.seconds[0]), "neutral": round(self.seconds[1]), "distracting": round(self.seconds[2]) },
            "unknown": self.unknown.iter().take(50).collect::<Vec<_>>(),
            "active": active,
            // How sure the sorter was that all this was work: experience goes by it.
            "sure": if self.seconds[0] > 0.0 { ((self.sure_seconds / self.seconds[0]).clamp(0.0, 1.0) * 100.0).round() / 100.0 } else { 1.0 },
            "in": most,
        });
        self.sure_seconds = 0.0;
        self.seconds = [0.0; 3];
        self.active = 0.0;
        self.by_program.clear();
        self.unknown.clear();
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn opening_the_panel_keeps_a_correction_target_without_counting_time_there() {
        let mut t=tracker();t.front(Some("code.exe"));t.pass(5.0,0.0);t.front(None);t.pass(30.0,0.0);
        assert_eq!(t.previous_classification(),Some(("code.exe",None,Some(Class::Productive))));
        assert_eq!(t.report()["seconds"]["productive"],5.0);assert!(t.program_name().is_none());
        assert!(Tracker::default().previous_classification().is_none());
    }

    fn tracker() -> Tracker {
        let mut tracker = Tracker::default();
        tracker.set_rules(&json!({ "Code.exe": "productive", "steam.exe": "distracting", "explorer.exe": "neutral", "odd.exe": "nonsense" }));
        tracker
    }

    #[test]
    fn time_goes_to_the_class_of_the_program_in_front() {
        let mut tracker = tracker();
        tracker.front(Some(r"C:\Program Files\Microsoft VS Code\Code.exe"));
        tracker.pass(30.0, 1.0);
        tracker.front(Some("steam.exe"));
        tracker.pass(12.5, 0.0);
        tracker.front(Some("explorer.exe"));
        tracker.pass(4.0, 0.0);
        let report = tracker.report();
        assert_eq!((&report["seconds"], &report["unknown"]), (&json!({ "productive": 30.0, "neutral": 4.0, "distracting": 12.5 }), &json!([])));
    }

    #[test]
    fn a_program_in_no_list_is_neutral_and_asked_about_once() {
        let mut tracker = tracker();
        tracker.front(Some(r"D:\Games\Foo.EXE"));
        tracker.pass(5.0, 0.0);
        tracker.pass(5.0, 0.0);
        // A rule with a class that is none of the three is no rule.
        tracker.front(Some("odd.exe"));
        tracker.pass(1.0, 0.0);
        let report = tracker.report();
        assert_eq!((&report["seconds"], &report["unknown"]), (&json!({ "productive": 0.0, "neutral": 11.0, "distracting": 0.0 }), &json!(["foo.exe", "odd.exe"])));
    }

    #[test]
    fn a_browser_s_title_is_its_page_and_other_programs_have_none() {
        assert_eq!(page("chrome.exe", "Rust ownership explained - YouTube - Google Chrome").as_deref(), Some("Rust ownership explained - YouTube"));
        assert_eq!(page("msedge.exe", "Inbox - Personal - Microsoft\u{200b} Edge").as_deref(), Some("Inbox - Personal"));
        assert_eq!(page("firefox.exe", "docs.rs \u{2014} Mozilla Firefox").as_deref(), Some("docs.rs"));
        assert_eq!(page("chrome.exe", "New Tab - Google Chrome"), None);
        assert_eq!(page("code.exe", "main.rs - desk - Visual Studio Code"), None);
        assert_eq!(page("safari", "Rust documentation — Safari").as_deref(), Some("Rust documentation"));
    }

    #[test]
    fn a_page_counts_as_what_it_was_found_to_be_and_until_then_as_its_browser() {
        let mut tracker = tracker();
        tracker.set_rules(&json!({ "chrome.exe": "neutral" }));
        // A page not met before is to be asked about, once.
        assert_eq!(tracker.front_page(Some("chrome.exe"), Some("Cat videos - YouTube - Google Chrome")).as_deref(), Some("Cat videos - YouTube"));
        tracker.pass(10.0, 0.0);
        assert_eq!(tracker.now(), Some(("Cat videos - YouTube".to_string(), Some(Class::Neutral))));
        tracker.learn("Cat videos - YouTube", Some(Class::Distracting), false, 1.0);
        tracker.pass(20.0, 0.0);
        assert_eq!(tracker.front_page(Some("chrome.exe"), Some("Cat videos - YouTube - Google Chrome")), None);
        assert_eq!(tracker.now(), Some(("Cat videos - YouTube".to_string(), Some(Class::Distracting))));
        // Another page in the same browser is its own matter; one nobody could say stays its browser's.
        assert!(tracker.front_page(Some("chrome.exe"), Some("std::rc - Rust - Google Chrome")).is_some());
        tracker.learn("std::rc - Rust", Some(Class::Productive), false, 1.0);
        tracker.pass(30.0, 0.0);
        assert!(tracker.front_page(Some("chrome.exe"), Some("Something odd - Google Chrome")).is_some());
        tracker.learn("Something odd", None, false, 0.0);
        tracker.pass(5.0, 0.0);
        assert_eq!(tracker.report()["seconds"], json!({ "productive": 30.0, "neutral": 15.0, "distracting": 20.0 }));
    }

    #[test]
    fn a_site_is_the_name_a_title_ends_with() {
        assert_eq!(site_of("Rust ownership explained - YouTube").as_deref(), Some("youtube"));
        assert_eq!(site_of("Pull request #4 \u{b7} acme/api \u{b7} GitHub").as_deref(), Some("github"));
        assert_eq!(site_of("How do I borrow twice? - Stack Overflow").as_deref(), Some("stack overflow"));
        assert_eq!(site_of("Inbox (3) - me@example.com - Gmail").as_deref(), Some("gmail"));
        // One part, an address, or a sentence is no site's name.
        assert_eq!(site_of("Netflix"), None);
        assert_eq!(site_of("Notes - C:\\Users\\me"), None);
        assert_eq!(site_of("Why - this is a whole sentence of a subtitle"), None);
    }

    #[test]
    fn a_site_known_for_what_it_is_is_not_asked_about_page_by_page() {
        let mut tracker = tracker();
        tracker.set_rules(&json!({ "chrome.exe": "neutral" }));
        // The first page of a site is asked about; the answer is for the whole site.
        assert!(tracker.front_page(Some("chrome.exe"), Some("Chapter 1 - Rustbook - Google Chrome")).is_some());
        tracker.learn("Chapter 1 - Rustbook", Some(Class::Productive), true, 1.0);
        assert_eq!(tracker.front_page(Some("chrome.exe"), Some("Chapter 2 - Rustbook - Google Chrome")), None);
        tracker.pass(10.0, 0.0);
        assert_eq!(tracker.sites(), vec![("rustbook".to_string(), Some(Class::Productive), false)]);

        // The user's own word on a site comes before anybody's, and its pages are not asked about at all.
        tracker.set_rules(&json!({ "chrome.exe": "neutral", "site:rustbook": "distracting", "site:forum": "productive" }));
        assert_eq!(tracker.front_page(Some("chrome.exe"), Some("Chapter 3 - Rustbook - Google Chrome")), None);
        tracker.pass(10.0, 0.0);
        assert_eq!(tracker.front_page(Some("chrome.exe"), Some("A thread - Forum - Google Chrome")), None);
        tracker.pass(5.0, 0.0);
        assert_eq!(tracker.report()["seconds"], json!({ "productive": 15.0, "neutral": 0.0, "distracting": 10.0 }));
        assert_eq!(tracker.sites(), vec![("forum".to_string(), Some(Class::Productive), true), ("rustbook".to_string(), Some(Class::Distracting), true)]);
        // A site's rule is not a program in a list: it is not reported as one in no list either.
        assert_eq!(tracker.report()["unknown"], json!([]));
    }

    #[test]
    fn a_report_says_how_much_of_the_work_was_worked_in_and_where() {
        let mut tracker = tracker();
        tracker.set_rules(&json!({ "code.exe": "productive", "word.exe": "productive", "steam.exe": "distracting" }));
        tracker.front(Some("code.exe"));
        // Six seconds with a touch in each two, then four with none since.
        for _ in 0..3 {
            tracker.pass(2.0, 0.5);
        }
        tracker.pass(2.0, 2.5);
        tracker.pass(2.0, 4.5);
        tracker.front(Some("word.exe"));
        tracker.pass(2.0, 0.1);
        // Time in what is not productive is neither here nor there.
        tracker.front(Some("steam.exe"));
        tracker.pass(8.0, 0.1);
        let report = tracker.report();
        assert_eq!((report["seconds"]["productive"].as_f64(), report["active"].as_f64(), report["in"].as_str()), (Some(12.0), Some(0.67), Some("code.exe")));
        // With no productive time there is nothing to have been idle in.
        let empty = tracker.report();
        assert_eq!((empty["active"].as_f64(), empty["in"].is_null()), (Some(1.0), true));
    }

    #[test]
    fn nobody_there_earns_nothing() {
        let mut tracker = tracker();
        tracker.front(Some("code.exe"));
        // Ten seconds passed and the last touch was 125 s ago: they went away 5 s before the end of it.
        tracker.pass(10.0, AWAY_AFTER + 5.0);
        // And nothing at all once they have been away for the whole of it.
        tracker.pass(10.0, AWAY_AFTER + 30.0);
        assert_eq!(tracker.report()["seconds"]["productive"], json!(5.0));
    }

    #[test]
    fn no_program_in_front_earns_nothing() {
        let mut tracker = tracker();
        tracker.front(None);
        tracker.pass(10.0, 0.0);
        assert!(!tracker.any());
    }

    #[test]
    fn a_report_starts_the_count_again() {
        let mut tracker = tracker();
        tracker.front(Some("code.exe"));
        tracker.pass(10.0, 0.0);
        assert!(tracker.any());
        tracker.report();
        assert!(!tracker.any());
        assert_eq!(tracker.report()["seconds"]["productive"], json!(0.0));
    }

    #[test]
    fn productive_time_is_reported_with_how_sure_the_sorter_was_of_it() {
        let mut tracker = Tracker::default();
        tracker.set_rules(&serde_json::json!({ "claude.exe": "productive", "code.exe": "productive", "chrome.exe": "neutral" }));
        tracker.set_sure(&serde_json::json!({ "claude.exe": 0.5, "code.exe": 1.0 }));
        // Half sure of all of it.
        tracker.front(Some("C:\\Apps\\claude.exe"));
        tracker.pass(30.0, 0.0);
        assert_eq!(tracker.report()["sure"], 0.5);
        // Half of it half sure, half of it wholly: three quarters.
        tracker.pass(30.0, 0.0);
        tracker.front(Some("code.exe"));
        tracker.pass(30.0, 0.0);
        assert_eq!(tracker.report()["sure"], 0.75);
        // A page counts by how sure the sorter was of the page, not of its browser.
        assert!(tracker.front_page(Some("chrome.exe"), Some("Maybe work - Google Chrome")).is_some());
        tracker.learn("Maybe work", Some(Class::Productive), false, 0.6);
        tracker.pass(10.0, 0.0);
        assert_eq!(tracker.report()["sure"], 0.6);
        // With no productive time there is nothing to be unsure of.
        assert_eq!(tracker.report()["sure"], 1.0);
    }
}
