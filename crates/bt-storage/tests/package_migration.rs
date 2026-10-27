use bt_core::{
    legacy_store::Read,
    legacy_xml::{Kind, Value},
    migration,
    package_snapshot::{PackageSnapshot, RawPackageEntry, RawPackageValue},
};
use bt_storage::{Outcome, package};
use std::{cell::Cell, fs, io};

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn entry(key: &str, value: RawPackageValue) -> RawPackageEntry {
    RawPackageEntry {
        key_utf16: units(key),
        value,
    }
}
fn snapshot() -> PackageSnapshot {
    PackageSnapshot {
        family: "xammen.Bettertrumpet_test".into(),
        container_existed: true,
        entries: vec![
            entry("IsExpanded", RawPackageValue::Bool { value: true }),
            entry(
                "ActiveThemeName",
                RawPackageValue::String {
                    utf16: units("Night"),
                },
            ),
            entry(
                "PeakMeterFps",
                RawPackageValue::String {
                    utf16: units("<?xml version=\"1.0\" encoding=\"utf-16\"?><int>60</int>"),
                },
            ),
            entry(
                "Hotkey",
                RawPackageValue::String {
                    utf16: units("not xml"),
                },
            ),
            entry(
                "Composite",
                RawPackageValue::Other {
                    property_type: None,
                },
            ),
            entry(
                "Number",
                RawPackageValue::Other {
                    property_type: Some(5),
                },
            ),
            // Unpaired surrogate survives the archive unchanged.
            entry(
                "Odd",
                RawPackageValue::String {
                    utf16: vec![0xd800, 0x41],
                },
            ),
        ],
    }
}

#[test]
fn reads_follow_the_original_package_bag() {
    let s = snapshot();
    assert_eq!(
        s.read("isexpanded", Kind::Bool),
        Read::Value(Value::Bool(true))
    );
    assert_eq!(s.read("IsExpanded", Kind::String), Read::Default);
    assert_eq!(
        s.read("ACTIVETHEMENAME", Kind::String),
        Read::Value(Value::String(Some("Night".into())))
    );
    assert_eq!(s.read("ActiveThemeName", Kind::Bool), Read::Default);
    assert_eq!(
        s.read("PeakMeterFps", Kind::I32),
        Read::Value(Value::I32(60))
    );
    // Package-bag XML errors escape the original getter.
    assert_eq!(s.read("Hotkey", Kind::Hotkey), Read::Throws);
    assert_eq!(s.read("Composite", Kind::String), Read::Default);
    assert_eq!(s.read("Number", Kind::I32), Read::Default);
    assert_eq!(s.read("Missing", Kind::Bool), Read::Default);
    assert!(s.has_key("number") && !s.has_key("Missing"));
}

#[test]
fn archives_round_trip_and_reject_invalid_snapshots() {
    let s = snapshot();
    let bytes = migration::encode_package(&s).unwrap();
    assert_eq!(migration::decode_package(&bytes).unwrap(), s);
    let mut duplicate = s.clone();
    duplicate
        .entries
        .push(entry("IsExpanded", RawPackageValue::Bool { value: false }));
    assert!(migration::encode_package(&duplicate).is_err());
    let missing = PackageSnapshot {
        container_existed: false,
        ..s.clone()
    };
    assert!(migration::encode_package(&missing).is_err());
    let registry = migration::encode_registry(&Default::default()).unwrap();
    assert!(migration::decode_package(&registry).is_err());
}

#[test]
fn first_run_archives_once_and_later_runs_skip_the_capture() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("rust");
    fs::create_dir_all(&dir).unwrap();
    let first = package(&dir, || Ok(snapshot())).unwrap();
    assert_eq!(first.outcome, Outcome::Created);
    let bytes = fs::read(&first.archive_path).unwrap();
    let captured = Cell::new(false);
    let second = package(&dir, || {
        captured.set(true);
        Err(io::Error::other("must not capture again"))
    })
    .unwrap();
    assert!(!captured.get());
    assert_eq!(second.outcome, Outcome::Existing);
    assert_eq!(second.snapshot, snapshot());
    assert_eq!(fs::read(&first.archive_path).unwrap(), bytes);
}
