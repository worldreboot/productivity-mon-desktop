//! Isolated CI probe, never shipped as the desktop app. Uses the same signed source and native helper.
#[path = "../src/updates/source.rs"]
mod source;
use std::{fs, path::PathBuf};
use velopack::{locator, UpdateCheck, UpdateManager, UpdateOptions};
fn main() {
    velopack::VelopackApp::build()
        .set_auto_apply_on_startup(false)
        .run();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(executable.with_file_name("fixture.json"))?)?;
    let root = PathBuf::from(config["root"].as_str().unwrap());
    let mut key = [0; 32];
    let hex = config["public_key"].as_str().unwrap();
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    let source = source::SignedSource::new(
        config["base_url"].as_str().unwrap().into(),
        key,
        source::CHANNEL.into(),
    );
    let manager = UpdateManager::new(
        source,
        Some(UpdateOptions {
            ExplicitChannel: Some(source::CHANNEL.into()),
            ..Default::default()
        }),
        None,
    )?;
    let version = manager.get_current_version_as_string();
    if version == "0.0.2" {
        assert_eq!(
            fs::read(root.join("profile/token"))?,
            b"fixture-account-kept"
        );
        assert_eq!(
            fs::read(root.join("profile/pending.jsonl"))?,
            b"fixture-unsent-activity\n"
        );
        fs::write(
            root.join("result.json"),
            b"{\"result\":\"updated\",\"version\":\"0.0.2\",\"profilePreserved\":true}",
        )?;
        return Ok(());
    }
    let outcome = (|| -> Result<(), velopack::Error> {
        let UpdateCheck::UpdateAvailable(update) = manager.check_for_updates()? else {
            return Err(velopack::Error::Other("No test update".into()));
        };
        manager.download_updates(&update, None)?;
        let locator = locator::auto_locate_app_manifest(locator::LocationContext::Unknown)?;
        source::verify_package(
            &locator
                .get_packages_dir()
                .join(&update.TargetFullRelease.FileName),
            &update.TargetFullRelease,
        )?;
        manager.wait_exit_then_apply_updates(
            &update.TargetFullRelease,
            true,
            true,
            Vec::<String>::new(),
        )?;
        Ok(())
    })();
    if let Err(error) = outcome {
        fs::write(
            root.join("result.json"),
            serde_json::to_vec(
                &serde_json::json!({"result":"rejected","version":version,"error":error.to_string()}),
            )?,
        )?;
    }
    Ok(())
}
