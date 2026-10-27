use bt_core::overrides::EncodedValue;
use bt_storage::editable::{EditableFile, Error, Outcome};
use std::{
    fs,
    sync::{Arc, Barrier},
};
fn key(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn set(file: &mut EditableFile, name: &str, value: &str) {
    file.document
        .set(&key(name), EncodedValue::Text(key(value)), |a, b| a == b)
        .unwrap();
}

#[test]
fn saves_restart_exact_values_without_touching_archives_or_source() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config/rust");
    fs::create_dir_all(dir.parent().unwrap()).unwrap();
    let source = dir.parent().unwrap().join("settings.json");
    fs::write(&source, b"{Original:'untouched'}").unwrap();
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("migration.v1.json"), b"immutable archive bytes").unwrap();
    let mut file = EditableFile::open(&dir).unwrap();
    assert!(!file.existed_on_open() && !file.path().exists());
    set(&mut file, "IsTelemetryEnabled", "<boolean>false</boolean>");
    file.document
        .set(
            &key("profile"),
            EncodedValue::Text(vec![0xd800, 0, 0xdc00]),
            |a, b| a == b,
        )
        .unwrap();
    assert_eq!(file.save().unwrap(), Outcome::Saved);
    assert!(!file.existed_on_open());
    let saved = fs::read(file.path()).unwrap();
    assert_eq!(file.save().unwrap(), Outcome::Unchanged);
    assert_eq!(fs::read(file.path()).unwrap(), saved);
    let reopened = EditableFile::open(&dir).unwrap();
    assert!(reopened.existed_on_open());
    assert_eq!(
        reopened
            .document
            .get(&key("profile"), |a, b| a == b)
            .unwrap(),
        Some(EncodedValue::Text(vec![0xd800, 0, 0xdc00]))
    );
    assert_eq!(fs::read(&source).unwrap(), b"{Original:'untouched'}");
    assert_eq!(
        fs::read(dir.join("migration.v1.json")).unwrap(),
        b"immutable archive bytes"
    );
}

#[test]
fn stale_and_concurrent_writers_conflict_without_losing_the_winner() {
    let temp = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|n| {
            let path = temp.path().to_owned();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut file = EditableFile::open(&path).unwrap();
                set(&mut file, &format!("key{n}"), "value");
                barrier.wait();
                file.save()
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(Outcome::Saved)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::Conflict)))
            .count(),
        1
    );
    let mut first = EditableFile::open(temp.path()).unwrap();
    let mut stale = EditableFile::open(temp.path()).unwrap();
    set(&mut first, "first", "1");
    first.save().unwrap();
    let saved = fs::read(first.path()).unwrap();
    set(&mut stale, "second", "2");
    assert!(matches!(stale.save(), Err(Error::Conflict)));
    assert_eq!(fs::read(first.path()).unwrap(), saved);
    assert_eq!(
        stale.document.get(&key("second"), |a, b| a == b).unwrap(),
        Some(EncodedValue::Text(key("2")))
    );
}

#[test]
fn external_edits_and_invalid_versions_are_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let mut file = EditableFile::open(temp.path()).unwrap();
    set(&mut file, "a", "1");
    file.save().unwrap();
    let future = br#"{"format":"BetterTrumpet.Rust.Settings","version":99,"entries":[]}"#;
    fs::write(file.path(), future).unwrap();
    set(&mut file, "a", "2");
    assert!(matches!(file.save(), Err(Error::Conflict)));
    assert!(EditableFile::open(temp.path()).is_err());
    assert_eq!(fs::read(file.path()).unwrap(), future);
}

#[test]
fn failed_saves_keep_memory_changes_and_leave_existing_files_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let mut file = EditableFile::open(temp.path()).unwrap();
    set(&mut file, "a", "1");
    file.save().unwrap();
    let saved = fs::read(file.path()).unwrap();
    // Block the sidecar lock path without relying on root/admin permission semantics.
    fs::remove_file(temp.path().join("settings.v1.lock")).unwrap();
    fs::create_dir(temp.path().join("settings.v1.lock")).unwrap();
    set(&mut file, "a", "2");
    assert!(file.save().is_err());
    assert_eq!(fs::read(file.path()).unwrap(), saved);
    assert_eq!(
        file.document.get(&key("a"), |a, b| a == b).unwrap(),
        Some(EncodedValue::Text(key("2")))
    );
}
