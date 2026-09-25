//! One-time, non-destructive import of Motrix Next data after the Rayburst identifier change.

use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager};

const LEGACY_APP_ID: &str = "com.motrix.next";
const CURRENT_APP_ID: &str = env!("DESKTOP_APP_ID");
const STORE_FILES: &[&str] = &[
    "config.json",
    "user.json",
    "system.json",
    "download.session",
];

fn legacy_sibling(current: &Path) -> Option<PathBuf> {
    let parent = current.parent()?;
    (current.file_name()?.to_str()? == CURRENT_APP_ID).then(|| parent.join(LEGACY_APP_ID))
}

fn copy_if_absent(source: &Path, destination: &Path) -> io::Result<bool> {
    if !source.is_file() || destination.exists() {
        return Ok(false);
    }
    if source
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        let input = std::fs::File::open(source)?;
        serde_json::from_reader::<_, serde_json::Value>(input)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("Missing target directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut input = std::fs::File::open(source)?;
    io::copy(&mut input, temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(true),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.error),
    }
}

fn backup_history_if_absent(
    source: &Path,
    destination: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !source.is_file() || destination.exists() {
        return Ok(false);
    }
    let parent = destination
        .parent()
        .ok_or("Missing history target directory")?;
    std::fs::create_dir_all(parent)?;
    let source_db = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    source_db.prepare("SELECT gid, added_at, meta FROM download_history LIMIT 0")?;
    let integrity: String = source_db.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(format!("Legacy history integrity check failed: {integrity}").into());
    }

    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut target_db = Connection::open(temporary.path())?;
        let backup = Backup::new(&source_db, &mut target_db)?;
        backup.run_to_completion(64, Duration::from_millis(10), None)?;
    }
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(true),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.error.into()),
    }
}

pub fn import_from_motrix_next(app: &AppHandle) {
    let Ok(data_dir) = app.path().app_data_dir() else {
        return;
    };
    if let Some(old_dir) = legacy_sibling(&data_dir) {
        for name in STORE_FILES {
            match copy_if_absent(&old_dir.join(name), &data_dir.join(name)) {
                Ok(true) => log::info!("legacy_data: imported {name}"),
                Ok(false) => {}
                Err(error) => log::warn!("legacy_data: {name} import failed: {error}"),
            }
        }
    }

    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    if let Some(old_dir) = legacy_sibling(&config_dir) {
        match backup_history_if_absent(&old_dir.join("history.db"), &config_dir.join("history.db"))
        {
            Ok(true) => log::info!("legacy_data: imported history.db"),
            Ok(false) => {}
            Err(error) => log::warn!("legacy_data: history.db import failed: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_imports_into_a_missing_destination() {
        let root = tempfile::tempdir().expect("temporary directory");
        let old = root.path().join(LEGACY_APP_ID);
        let new = root.path().join(CURRENT_APP_ID);
        std::fs::create_dir_all(&old).expect("old directory");
        let source = old.join("config.json");
        let destination = new.join("config.json");
        std::fs::write(&source, "{\"preferences\":{}}").expect("old config");
        assert_eq!(legacy_sibling(&new).as_deref(), Some(old.as_path()));
        assert!(copy_if_absent(&source, &destination).expect("first import"));
        std::fs::write(&destination, "new").expect("updated config");
        assert!(!copy_if_absent(&source, &destination).expect("second import"));
        assert_eq!(std::fs::read_to_string(destination).expect("config"), "new");
    }

    #[test]
    fn invalid_legacy_json_does_not_create_a_new_store() {
        let root = tempfile::tempdir().expect("temporary directory");
        let source = root.path().join("old.json");
        let destination = root.path().join("new.json");
        std::fs::write(&source, "not json").expect("old config");
        assert_eq!(
            copy_if_absent(&source, &destination)
                .expect_err("invalid config must not migrate")
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(!destination.exists());
    }

    #[test]
    fn sqlite_backup_preserves_existing_history_and_does_not_overwrite() {
        let root = tempfile::tempdir().expect("temporary directory");
        let source = root.path().join("legacy.db");
        let destination = root.path().join("history.db");
        {
            let db = Connection::open(&source).expect("legacy database");
            db.execute_batch("CREATE TABLE download_history (gid TEXT, added_at TEXT, meta TEXT); INSERT INTO download_history VALUES ('g1', 'today', '{}');")
                .expect("legacy record");
        }
        assert!(backup_history_if_absent(&source, &destination).expect("backup"));
        assert!(!backup_history_if_absent(&source, &destination).expect("second backup"));
        let db = Connection::open(destination).expect("imported database");
        let gid: String = db
            .query_row("SELECT gid FROM download_history", [], |row| row.get(0))
            .expect("record");
        assert_eq!(gid, "g1");
    }
}
