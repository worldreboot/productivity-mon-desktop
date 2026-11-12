//! Network and package work never runs on the rendering thread. Installing requires a saved-activity barrier.
pub mod source;
use serde::Serialize;
use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::Duration,
};
use velopack::{locator, UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions};

#[derive(Clone, Serialize)]
pub struct Status {
    pub phase: &'static str,
    pub current: String,
    pub version: String,
    pub progress: i16,
    pub message: String,
}
impl Status {
    fn new(phase: &'static str, current: String, message: &str) -> Self {
        Self {
            phase,
            current,
            version: String::new(),
            progress: 0,
            message: message.into(),
        }
    }
}
enum Command {
    Check,
    Install(bool),
}
pub enum Event {
    Status(Status),
    Exit,
}
pub struct Updater {
    commands: Option<Sender<Command>>,
    events: Receiver<Event>,
    pub status: Status,
}

impl Updater {
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        let source = source::SignedSource::production();
        let manager = UpdateManager::new(
            source,
            Some(UpdateOptions {
                ExplicitChannel: Some(source::CHANNEL.into()),
                AllowVersionDowngrade: false,
                MaximumDeltasBeforeFallback: 10,
            }),
            None,
        );
        let Ok(manager) = manager else {
            return Self { commands:None, events:receive, status:Status::new("unavailable",String::new(),"Automatic updates are available in the new installer. Development copies are unchanged.") };
        };
        let status = Status::new(
            "checking",
            manager.get_current_version_as_string(),
            "Checking for updates…",
        );
        std::thread::spawn(move || run(manager, rx, events, wake));
        Self {
            commands: Some(tx),
            events: receive,
            status,
        }
    }
    pub fn check(&self) {
        if let Some(tx) = &self.commands {
            let _ = tx.send(Command::Check);
        }
    }
    pub fn ready(&self) -> bool {
        self.status.phase == "ready"
    }
    pub fn install(&self, restart: bool) {
        if let Some(tx) = &self.commands {
            let _ = tx.send(Command::Install(restart));
        }
    }
    /// Returns true only after the helper has successfully started and is waiting for this process to exit.
    pub fn poll(&mut self) -> (bool, bool) {
        let mut changed = false;
        let mut exit = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Status(status) => {
                    self.status = status;
                    changed = true;
                }
                Event::Exit => exit = true,
            }
        }
        (changed, exit)
    }
}

fn run(manager: UpdateManager, rx: Receiver<Command>, tx: Sender<Event>, wake: impl Fn() + Sync) {
    let current = manager.get_current_version_as_string();
    let send = |status: Status| {
        let _ = tx.send(Event::Status(status));
        wake();
    };
    let mut pending: Option<Box<UpdateInfo>> = None;
    let mut next = Duration::ZERO;
    loop {
        let command = match rx.recv_timeout(next) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => Command::Check,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match command {
            Command::Check => {
                if pending.is_some() {
                    next = Duration::from_secs(6 * 3600);
                    continue;
                }
                send(Status::new(
                    "checking",
                    current.clone(),
                    "Checking for updates…",
                ));
                let result = (|| -> Result<(), velopack::Error> {
                    match manager.check_for_updates()? {
                        UpdateCheck::UpdateAvailable(update) => {
                            let version = update.TargetFullRelease.Version.clone();
                            let mut status = Status::new(
                                "downloading",
                                current.clone(),
                                "Downloading in the background…",
                            );
                            status.version = version.clone();
                            send(status.clone());
                            let (progress, percent) = mpsc::channel();
                            // Scoped reporter ends when the download drops its sender, including on errors.
                            std::thread::scope(|scope| {
                                let report = &send;
                                scope.spawn(move || {
                                    while let Ok(value) = percent.recv() {
                                        let mut s = status.clone();
                                        s.progress = value;
                                        report(s);
                                    }
                                });
                                manager.download_updates(&update, Some(progress))
                            })?;
                            let paths = locator::auto_locate_app_manifest(
                                locator::LocationContext::Unknown,
                            )?;
                            source::verify_package(
                                &paths
                                    .get_packages_dir()
                                    .join(&update.TargetFullRelease.FileName),
                                &update.TargetFullRelease,
                            )?;
                            let mut status = Status::new(
                                "ready",
                                current.clone(),
                                "Ready to install when you restart or quit.",
                            );
                            status.version = version;
                            status.progress = 100;
                            send(status);
                            pending = Some(update);
                        }
                        _ => send(Status::new(
                            "current",
                            current.clone(),
                            "You’re up to date.",
                        )),
                    }
                    Ok(())
                })();
                next = if result.is_ok() {
                    Duration::from_secs(6 * 3600)
                } else {
                    Duration::from_secs(30 * 60)
                };
                if let Err(error) = result {
                    eprintln!("[updates] {error}");
                    send(Status::new(
                        "error",
                        current.clone(),
                        "Couldn’t update. Your current version still works. Try again later.",
                    ));
                }
            }
            Command::Install(restart) => {
                let Some(update) = pending.as_ref() else {
                    continue;
                };
                send(Status::new(
                    "installing",
                    current.clone(),
                    "Installing update…",
                ));
                let result = (|| -> Result<(), velopack::Error> {
                    // Refresh the signed feed before applying: a retracted/expired release must not install.
                    let feed = manager.get_release_feed()?;
                    if !feed.Assets.iter().any(|asset| {
                        serde_json::to_vec(asset).ok()
                            == serde_json::to_vec(&update.TargetFullRelease).ok()
                    }) {
                        return Err(velopack::Error::InvalidPackage(
                            "Update was withdrawn".into(),
                        ));
                    }
                    let paths =
                        locator::auto_locate_app_manifest(locator::LocationContext::Unknown)?;
                    source::verify_package(
                        &paths
                            .get_packages_dir()
                            .join(&update.TargetFullRelease.FileName),
                        &update.TargetFullRelease,
                    )?;
                    // No stale --models or test arguments: updated bundles locate their own resources.
                    manager.wait_exit_then_apply_updates(
                        &update.TargetFullRelease,
                        true,
                        restart,
                        Vec::<String>::new(),
                    )
                })();
                match result {
                    Ok(()) => {
                        let _ = tx.send(Event::Exit);
                        wake();
                        return;
                    }
                    Err(error) => {
                        eprintln!("[updates] install refused: {error}");
                        pending = None;
                        send(Status::new("error",current.clone(),"Couldn’t install the update. Your activity is saved; try checking again."));
                        next = Duration::from_secs(30 * 60);
                    }
                }
            }
        }
    }
}
