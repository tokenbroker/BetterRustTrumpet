#[path = "../../bt-core/tests/support/mod.rs"]
mod support;
use bt_core::{
    legacy_store::{PortableLoadStatus, Read},
    legacy_xml::{Kind, Value},
};
use bt_storage::{self as storage, Outcome};
use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
};

fn setup(root: &Path, bytes: Option<&[u8]>) {
    fs::write(root.join("portable.marker"), b"").unwrap();
    fs::create_dir(root.join("config")).unwrap();
    if let Some(bytes) = bytes {
        fs::write(root.join("config/settings.json"), bytes).unwrap();
    }
}
fn archive(root: &Path) -> std::path::PathBuf {
    root.join("config/rust/migration.v1.json")
}

#[test]
fn first_run_preserves_the_full_bag_and_second_run_uses_the_archive() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = b"\xef\xbb\xbf{/*unknown*/'Addon.key':{'data':[1,null]},'Volume':1,'Volume':2,'TelemetryEnabled':false}";
    setup(root, Some(source));
    let first = storage::portable(root, &support::Central).unwrap();
    assert_eq!(first.outcome, Outcome::Created);
    assert_eq!(first.snapshot.original_bytes(), Some(source.as_slice()));
    assert!(first.snapshot.store().has_key("Addon.key"));
    assert_eq!(
        first.snapshot.store().read("Volume", Kind::I32),
        Read::Value(Value::I32(2))
    );
    assert_eq!(
        first.snapshot.store().read("TelemetryEnabled", Kind::Bool),
        Read::Value(Value::Bool(false))
    );
    assert_eq!(fs::read(root.join("config/settings.json")).unwrap(), source);
    let saved = fs::read(archive(root)).unwrap();
    fs::write(root.join("config/settings.json"), b"{Volume:99}").unwrap();
    let second = storage::portable(root, &support::Central).unwrap();
    assert_eq!(second.outcome, Outcome::Existing);
    assert_eq!(second.snapshot, first.snapshot);
    assert_eq!(fs::read(archive(root)).unwrap(), saved);
    assert_eq!(
        fs::read(root.join("config/settings.json")).unwrap(),
        b"{Volume:99}"
    );
}

#[test]
fn concurrent_initializers_install_one_complete_archive() {
    let temp = tempfile::tempdir().unwrap();
    setup(temp.path(), Some(b"{unknown:'keep',Volume:42}"));
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let root = temp.path().to_owned();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                storage::portable(&root, &support::Central).unwrap()
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
    assert_eq!(
        fs::read_dir(temp.path().join("config/rust"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn missing_and_corrupt_sources_remain_recoverable_without_reimport() {
    for (source, status) in [
        (None, PortableLoadStatus::Missing),
        (Some(b"{broken\xff".as_slice()), PortableLoadStatus::Invalid),
    ] {
        let temp = tempfile::tempdir().unwrap();
        setup(temp.path(), source);
        let first = storage::portable(temp.path(), &support::Central).unwrap();
        assert_eq!(first.snapshot.status(), status);
        assert_eq!(first.snapshot.original_bytes(), source);
        assert_eq!(
            temp.path().join("config/settings.json").exists(),
            source.is_some()
        );
        let next = storage::portable(temp.path(), &support::Central).unwrap();
        assert_eq!(next.outcome, Outcome::Existing);
        assert_eq!(next.snapshot, first.snapshot);
    }
}

#[test]
fn bad_existing_archives_are_never_replaced() {
    for bytes in [
        b"{broken".as_slice(),
        br#"{"format":"future","version":99,"original_bytes":null}"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        setup(temp.path(), Some(b"{Volume:42}"));
        fs::create_dir(temp.path().join("config/rust")).unwrap();
        fs::write(archive(temp.path()), bytes).unwrap();
        assert!(storage::portable(temp.path(), &support::Central).is_err());
        assert_eq!(fs::read(archive(temp.path())).unwrap(), bytes);
        assert_eq!(
            fs::read(temp.path().join("config/settings.json")).unwrap(),
            b"{Volume:42}"
        );
    }
}

#[test]
fn failed_source_reads_and_destination_creation_do_not_modify_input() {
    let temp = tempfile::tempdir().unwrap();
    setup(temp.path(), None);
    fs::create_dir(temp.path().join("config/settings.json")).unwrap();
    assert!(storage::portable(temp.path(), &support::Central).is_err());
    assert!(!archive(temp.path()).exists());
    let temp = tempfile::tempdir().unwrap();
    setup(temp.path(), Some(b"{Volume:42}"));
    fs::write(temp.path().join("config/rust"), b"occupied").unwrap();
    assert!(storage::portable(temp.path(), &support::Central).is_err());
    assert_eq!(
        fs::read(temp.path().join("config/rust")).unwrap(),
        b"occupied"
    );
    assert_eq!(
        fs::read(temp.path().join("config/settings.json")).unwrap(),
        b"{Volume:42}"
    );
}

#[test]
fn marker_directories_do_not_enable_portable_mode() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("portable.marker")).unwrap();
    assert!(matches!(
        storage::portable(temp.path(), &support::Central),
        Err(storage::Error::NotPortable)
    ));
    assert!(!temp.path().join("config").exists());
    fs::write(temp.path().join(".portable"), b"").unwrap();
    assert_eq!(
        storage::portable(temp.path(), &support::Central)
            .unwrap()
            .outcome,
        Outcome::Created
    );
}

#[test]
fn an_archive_alias_to_the_source_cannot_overwrite_the_source() {
    let temp = tempfile::tempdir().unwrap();
    setup(temp.path(), Some(b"{Unknown:'original'}"));
    fs::create_dir(temp.path().join("config/rust")).unwrap();
    fs::hard_link(
        temp.path().join("config/settings.json"),
        archive(temp.path()),
    )
    .unwrap();
    assert!(storage::portable(temp.path(), &support::Central).is_err());
    assert_eq!(
        fs::read(temp.path().join("config/settings.json")).unwrap(),
        b"{Unknown:'original'}"
    );
    assert_eq!(
        fs::read(archive(temp.path())).unwrap(),
        b"{Unknown:'original'}"
    );
}

#[test]
fn abandoned_staging_files_are_ignored_and_preserved() {
    let temp = tempfile::tempdir().unwrap();
    setup(temp.path(), Some(b"{Volume:42}"));
    fs::create_dir(temp.path().join("config/rust")).unwrap();
    let abandoned = temp.path().join("config/rust/.bt-migration-interrupted");
    fs::write(&abandoned, b"partial old attempt").unwrap();
    let result = storage::portable(temp.path(), &support::Central).unwrap();
    assert_eq!(result.outcome, Outcome::Created);
    assert_eq!(
        result.snapshot.store().read("Volume", Kind::I32),
        Read::Value(Value::I32(42))
    );
    assert_eq!(fs::read(abandoned).unwrap(), b"partial old attempt");
}
