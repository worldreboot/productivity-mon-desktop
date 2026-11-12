//! Activity is owned by an account even while that account is signed out.
use std::{fs, io::{self, Write}, path::{Path, PathBuf}};

fn folder(home: &Path, account: &str) -> io::Result<PathBuf> {
    let id = uuid::Uuid::parse_str(account).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid account ID for pending activity"))?;
    Ok(home.join("pending-reports").join(id.to_string()))
}

pub fn append(home: &Path, account: &str, report: &str) -> io::Result<()> {
    let directory = folder(home, account)?;
    fs::create_dir_all(&directory)?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(directory.join("1-active.jsonl"))?;
    writeln!(file, "{report}")?;
    file.sync_all()
}

pub fn batches(home: &Path, account: &str) -> io::Result<Vec<PathBuf>> {
    let directory = folder(home, account)?;
    let mut paths = match fs::read_dir(directory) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?.into_iter().filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl")).map(|entry|entry.path()).collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    paths.sort();
    Ok(paths)
}

/// Move the alpha.6 single-account queue once, without copying or guessing its owner.
pub fn migrate(home: &Path) -> io::Result<()> {
    let legacy = home.join("reports.jsonl");
    if !legacy.try_exists()? { return Ok(()); }
    let old = fs::read_to_string(home.join("account-id")).ok();
    let target = match old.as_deref().map(str::trim).and_then(|id|folder(home,id).ok()) {
        Some(directory) => {fs::create_dir_all(&directory)?; directory.join(format!("0-legacy-{}.jsonl",uuid::Uuid::new_v4()))},
        None => home.join(format!("reports-before-sign-in-{}.jsonl",uuid::Uuid::new_v4())),
    };
    fs::rename(legacy,target)
}

pub fn select(home: &Path, account: &str) -> io::Result<()> {
    folder(home, account)?;
    migrate(home)?;
    fs::write(home.join("account-id"),account)
}

/// No settings, exclusion lists, or per-account outboxes are removed.
pub fn sign_out(home: &Path) -> io::Result<()> {
    migrate(home)?;
    for name in ["desk.json","rules.json","legacy-token","account-id","token"] {
        match fs::remove_file(home.join(name)) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {let path=std::env::temp_dir().join(format!("productivitymon-outbox-{}",uuid::Uuid::new_v4()));fs::create_dir(&path).unwrap();path}
    #[test]
    fn switching_preserves_each_accounts_pending_activity_and_resumes_its_own_queue() {
        let home=fixture();let a=uuid::Uuid::new_v4().to_string();let b=uuid::Uuid::new_v4().to_string();
        select(&home,&a).unwrap();append(&home,&a,"a's unsent report").unwrap();
        fs::write(home.join("token"),"old credential").unwrap();fs::write(home.join("desk.json"),"old team").unwrap();
        fs::write(home.join("private.json"),"private settings").unwrap();
        sign_out(&home).unwrap();
        assert!(!home.join("token").exists());assert!(!home.join("desk.json").exists());assert!(home.join("private.json").exists());
        select(&home,&b).unwrap();assert!(batches(&home,&b).unwrap().is_empty());
        append(&home,&b,"b's report").unwrap();
        select(&home,&a).unwrap();let paths=batches(&home,&a).unwrap();assert_eq!(paths.len(),1);
        assert_eq!(fs::read_to_string(&paths[0]).unwrap(),"a's unsent report\n");
        assert_eq!(fs::read_to_string(&batches(&home,&b).unwrap()[0]).unwrap(),"b's report\n");
    }
    #[test]
    fn legacy_queue_is_attributed_to_its_previous_account_before_switching() {
        let home=fixture();let a=uuid::Uuid::new_v4().to_string();let b=uuid::Uuid::new_v4().to_string();
        fs::write(home.join("account-id"),&a).unwrap();fs::write(home.join("reports.jsonl"),"legacy activity").unwrap();
        select(&home,&b).unwrap();assert!(batches(&home,&b).unwrap().is_empty());
        assert_eq!(fs::read_to_string(&batches(&home,&a).unwrap()[0]).unwrap(),"legacy activity");
        migrate(&home).unwrap();assert_eq!(batches(&home,&a).unwrap().len(),1);
    }
    #[test]
    fn unowned_legacy_activity_is_never_assigned_to_a_new_account() {
        let home=fixture();fs::write(home.join("reports.jsonl"),"unowned").unwrap();
        select(&home,&uuid::Uuid::new_v4().to_string()).unwrap();
        assert!(fs::read_dir(&home).unwrap().filter_map(Result::ok).any(|e|e.file_name().to_string_lossy().starts_with("reports-before-sign-in-")));
        assert!(append(&home,"../other-account","unsafe").is_err());
    }
    #[test]
    fn failed_migration_does_not_remove_the_session_or_queued_activity() {
        let home=fixture();fs::write(home.join("account-id"),uuid::Uuid::new_v4().to_string()).unwrap();
        fs::write(home.join("token"),"credential").unwrap();fs::write(home.join("reports.jsonl"),"pending").unwrap();fs::write(home.join("pending-reports"),"blocked directory").unwrap();
        assert!(sign_out(&home).is_err());assert!(home.join("token").exists());assert_eq!(fs::read_to_string(home.join("reports.jsonl")).unwrap(),"pending");
    }
}
