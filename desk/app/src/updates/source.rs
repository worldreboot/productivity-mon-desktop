//! Signed feeds bind package hashes, app identity, channel and expiry to a pinned release key.
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::Path,
    sync::{mpsc::Sender, Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use velopack::{bundle::Manifest, sources::UpdateSource, Error, VelopackAsset, VelopackAssetFeed};

pub const APP_ID: &str = "ProductivityMon";
pub const BASE_URL: &str =
    "https://github.com/worldreboot/productivity-mon-desktop/releases/download/desktop-alpha";
pub const CHANNEL: &str = if cfg!(target_os = "macos") {
    "osx-alpha"
} else {
    "win-alpha"
};
const MAX_FEED: u64 = 4 * 1024 * 1024;
const MAX_PACKAGE: u64 = 2 * 1024 * 1024 * 1024;

fn invalid(message: &str) -> Error {
    Error::InvalidPackage(message.into())
}

#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}
#[derive(Deserialize)]
struct Payload {
    schema: u32,
    app_id: String,
    channel: String,
    expires: u64,
    feed: VelopackAssetFeed,
}

pub fn public_key() -> [u8; 32] {
    let hex = include_str!("../../../updater/public-key.hex").trim();
    let mut bytes = [0; 32];
    assert_eq!(hex.len(), 64, "invalid pinned update key");
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex update key");
    }
    bytes
}

fn safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 200
        && name.ends_with(".nupkg")
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
}

pub fn verify_feed(
    bytes: &[u8],
    key: &[u8; 32],
    channel: &str,
    now: u64,
) -> Result<VelopackAssetFeed, Error> {
    if bytes.len() as u64 > MAX_FEED {
        return Err(invalid("Update feed is too large"));
    }
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let payload = STANDARD
        .decode(envelope.payload)
        .map_err(|_| invalid("Invalid update payload"))?;
    let signature = STANDARD
        .decode(envelope.signature)
        .map_err(|_| invalid("Invalid update signature"))?;
    let key = VerifyingKey::from_bytes(key).map_err(|_| invalid("Invalid update key"))?;
    let signature =
        Signature::from_slice(&signature).map_err(|_| invalid("Invalid update signature"))?;
    key.verify_strict(&payload, &signature)
        .map_err(|_| invalid("Update signature did not verify"))?;
    let payload: Payload = serde_json::from_slice(&payload)?;
    if payload.schema != 1
        || payload.app_id != APP_ID
        || payload.channel != channel
        || payload.expires <= now
    {
        return Err(invalid(
            "Update feed identity, channel or expiry did not verify",
        ));
    }
    let mut names = std::collections::HashSet::new();
    for asset in &payload.feed.Assets {
        if asset.PackageId != APP_ID
            || !safe_filename(&asset.FileName)
            || !names.insert(&asset.FileName)
            || asset.Size == 0
            || asset.Size > MAX_PACKAGE
            || asset.SHA256.len() != 64
            || !asset.SHA256.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(asset.Type.as_str(), "Full" | "Delta")
            || semver::Version::parse(&asset.Version).is_err()
        {
            return Err(invalid("Invalid update asset"));
        }
    }
    Ok(payload.feed)
}

pub fn verify_package(path: &Path, asset: &VelopackAsset) -> Result<(), Error> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() != asset.Size {
        return Err(invalid("Update package size did not verify"));
    }
    let mut hash = Sha256::new();
    let mut reader = std::io::BufReader::new(file);
    let mut block = [0; 65536];
    loop {
        let count = reader.read(&mut block)?;
        if count == 0 {
            break;
        }
        hash.update(&block[..count]);
    }
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&asset.SHA256) {
        return Err(invalid("Update package hash did not verify"));
    }
    Ok(())
}

#[derive(Clone)]
pub struct SignedSource {
    base: String,
    key: [u8; 32],
    channel: String,
    agent: ureq::Agent,
    approved: Arc<Mutex<HashMap<String, VelopackAsset>>>,
}

impl SignedSource {
    pub fn production() -> Self {
        Self::new(BASE_URL.into(), public_key(), CHANNEL.into())
    }
    pub fn new(base: String, key: [u8; 32], channel: String) -> Self {
        Self {
            base,
            key,
            channel,
            approved: Default::default(),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(15))
                .timeout_read(Duration::from_secs(60))
                .timeout_write(Duration::from_secs(30))
                .build(),
        }
    }
    pub fn fetch(&self) -> Result<VelopackAssetFeed, Error> {
        let response = self
            .agent
            .get(&format!(
                "{}/releases.{}.signed.json",
                self.base, self.channel
            ))
            .set("Cache-Control", "no-cache")
            .call()
            .map_err(|_| Error::Other("Could not contact the update server".into()))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_FEED + 1)
            .read_to_end(&mut bytes)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid("Check your computer clock"))?
            .as_secs();
        let feed = verify_feed(&bytes, &self.key, &self.channel, now)?;
        *self
            .approved
            .lock()
            .map_err(|_| invalid("Update lock failed"))? = feed
            .Assets
            .iter()
            .map(|a| (a.FileName.clone(), a.clone()))
            .collect();
        Ok(feed)
    }
}

impl UpdateSource for SignedSource {
    fn get_release_feed(
        &self,
        channel: &str,
        app: &Manifest,
        _: &str,
    ) -> Result<VelopackAssetFeed, Error> {
        if channel != self.channel || app.id != APP_ID {
            return Err(invalid("Wrong update channel or app"));
        }
        self.fetch()
    }
    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local: &Path,
        progress: Option<Sender<i16>>,
    ) -> Result<(), Error> {
        let approved = self
            .approved
            .lock()
            .map_err(|_| invalid("Update lock failed"))?
            .get(&asset.FileName)
            .cloned();
        let Some(approved) = approved else {
            return Err(invalid("Unsigned update asset"));
        };
        if serde_json::to_vec(&approved)? != serde_json::to_vec(asset)? {
            return Err(invalid("Update asset changed"));
        }
        let result = (|| {
            let response = self
                .agent
                .get(&format!("{}/{}", self.base, asset.FileName))
                .call()
                .map_err(|_| Error::Other("Could not download the update".into()))?;
            let mut reader = response.into_reader().take(asset.Size + 1);
            let mut file = fs::File::create(local)?;
            let mut done = 0;
            let mut last = -1;
            let mut block = [0; 65536];
            loop {
                let count = reader.read(&mut block)?;
                if count == 0 {
                    break;
                }
                done += count as u64;
                if done > asset.Size {
                    return Err(invalid("Update download exceeded its signed size"));
                }
                file.write_all(&block[..count])?;
                let percent = (done * 100 / asset.Size) as i16;
                if percent != last {
                    if let Some(tx) = &progress {
                        let _ = tx.send(percent);
                    }
                    last = percent;
                }
            }
            file.sync_all()?;
            drop(file);
            verify_package(local, asset)
        })();
        if result.is_err() {
            let _ = fs::remove_file(local);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    fn asset() -> VelopackAsset {
        VelopackAsset {
            PackageId: APP_ID.into(),
            Version: "0.1.0-alpha.9".into(),
            Type: "Full".into(),
            FileName: "ProductivityMon-0.1.0-alpha.9-full.nupkg".into(),
            SHA256: format!("{:x}", Sha256::digest(b"good")),
            Size: 4,
            ..Default::default()
        }
    }
    fn signed(key: &SigningKey, payload: serde_json::Value) -> Vec<u8> {
        let payload = serde_json::to_vec(&payload).unwrap();
        serde_json::to_vec(&serde_json::json!({"payload":STANDARD.encode(&payload),"signature":STANDARD.encode(key.sign(&payload).to_bytes())})).unwrap()
    }
    fn payload() -> serde_json::Value {
        serde_json::json!({"schema":1,"app_id":APP_ID,"channel":"win-alpha","expires":200,"feed":{"Assets":[asset()]}})
    }
    #[test]
    fn signatures_reject_tampering_wrong_keys_and_unsigned_feeds() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let bytes = signed(&key, payload());
        assert_eq!(
            verify_feed(&bytes, &public, "win-alpha", 100)
                .unwrap()
                .Assets
                .len(),
            1
        );
        assert!(verify_feed(
            &bytes,
            &SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes(),
            "win-alpha",
            100
        )
        .is_err());
        let mut envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        envelope["payload"] = STANDARD.encode(b"modified").into();
        assert!(verify_feed(
            &serde_json::to_vec(&envelope).unwrap(),
            &public,
            "win-alpha",
            100
        )
        .is_err());
        assert!(verify_feed(b"{\"Assets\":[]}", &public, "win-alpha", 100).is_err());
    }
    #[test]
    fn signed_feeds_still_reject_expiry_cross_platform_and_unsafe_assets() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        assert!(verify_feed(&signed(&key, payload()), &public, "osx-alpha", 100).is_err());
        assert!(verify_feed(&signed(&key, payload()), &public, "win-alpha", 200).is_err());
        for field in ["PackageId", "Version", "FileName", "SHA256", "Type"] {
            let mut p = payload();
            p["feed"]["Assets"][0][field] = "../untrusted".into();
            assert!(
                verify_feed(&signed(&key, p), &public, "win-alpha", 100).is_err(),
                "{field}"
            );
        }
        for name in [
            "/a.nupkg",
            "https://evil/a.nupkg",
            "a\\b.nupkg",
            "a%2fb.nupkg",
            "../a.nupkg",
        ] {
            let mut p = payload();
            p["feed"]["Assets"][0]["FileName"] = name.into();
            assert!(verify_feed(&signed(&key, p), &public, "win-alpha", 100).is_err());
        }
    }
    #[test]
    fn cached_packages_are_reverified_before_installing() {
        let file = std::env::temp_dir().join(format!("update-hash-{}", uuid::Uuid::new_v4()));
        fs::write(&file, b"good").unwrap();
        assert!(verify_package(&file, &asset()).is_ok());
        fs::write(&file, b"evil").unwrap();
        assert!(verify_package(&file, &asset()).is_err());
        fs::write(&file, b"short").unwrap();
        assert!(verify_package(&file, &asset()).is_err());
        fs::remove_file(file).unwrap();
    }
}
