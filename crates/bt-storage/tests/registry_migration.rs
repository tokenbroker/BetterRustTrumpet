use bt_core::registry_snapshot::{RawRegistryValue, RegistrySnapshot};
use bt_storage::{self as storage, Outcome};
use std::{
    fs, io,
    sync::{Arc, Barrier},
};

fn sample(byte: u8) -> RegistrySnapshot {
    RegistrySnapshot {
        key_existed: true,
        values: vec![RawRegistryValue {
            name_utf16: vec![0xd800],
            kind: 12345,
            bytes: vec![byte, 0, 255],
        }],
    }
}

#[test]
fn registry_archives_preserve_raw_values_and_bypass_future_capture() {
    let temp = tempfile::tempdir().unwrap();
    let snapshot = sample(42);
    let first = storage::registry(temp.path(), || Ok(snapshot.clone())).unwrap();
    assert_eq!(first.outcome, Outcome::Created);
    assert_eq!(first.snapshot, snapshot);
    let bytes = fs::read(&first.archive_path).unwrap();
    let second = storage::registry(temp.path(), || {
        panic!("existing archive must bypass native source")
    })
    .unwrap();
    assert_eq!(second.outcome, Outcome::Existing);
    assert_eq!(second.snapshot, snapshot);
    assert_eq!(fs::read(second.archive_path).unwrap(), bytes);
}

#[test]
fn capture_failure_and_corrupt_archive_are_recoverable_without_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    assert!(
        storage::registry(temp.path(), || Err(io::Error::from(
            io::ErrorKind::PermissionDenied
        )))
        .is_err()
    );
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    let path = temp.path().join("migration.v1.json");
    fs::write(&path, b"{broken").unwrap();
    assert!(
        storage::registry(temp.path(), || panic!(
            "invalid archive must not silently reimport"
        ))
        .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), b"{broken");
}

#[test]
fn concurrent_registry_captures_keep_the_complete_winning_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|n| {
            let path = temp.path().to_owned();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                storage::registry(&path, || {
                    barrier.wait();
                    Ok(sample(n))
                })
                .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        results
            .iter()
            .filter(|r| r.outcome == Outcome::Created)
            .count(),
        1
    );
    for result in &results {
        assert_eq!(result.snapshot, results[0].snapshot);
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}
