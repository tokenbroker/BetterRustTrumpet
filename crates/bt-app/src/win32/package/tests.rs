//! Native, read-only tests against packages installed on this machine.
use super::*;
use bt_core::legacy_xml::Kind;
use std::path::PathBuf;

fn packages_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA")).join("Packages")
}

#[test]
fn family_names_match_windows_package_identity() {
    let calculator = family_name(
        "Microsoft.WindowsCalculator",
        "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US",
    )
    .unwrap();
    assert_eq!(calculator, "Microsoft.WindowsCalculator_8wekyb3d8bbwe");
    let family = legacy_family().unwrap();
    let (name, publisher_id) = family.split_once('_').unwrap();
    assert_eq!(name, PACKAGE_NAME);
    assert_eq!(publisher_id.len(), 13);
}

#[test]
fn missing_families_are_reported_without_creating_anything() {
    let family = "BetterTrumpetRustTests.NotInstalled_0000000000000";
    let snapshot = capture(family).unwrap();
    assert!(!snapshot.container_existed && snapshot.entries.is_empty());
    assert!(!packages_dir().join(family).exists());
}

/// Finds an installed package with LocalSettings values; the capture must be
/// stable, complete and agree with direct WinRT lookups.
#[test]
fn installed_package_settings_are_captured_losslessly() {
    let mut checked = 0;
    for dir in std::fs::read_dir(packages_dir())
        .unwrap()
        .flatten()
        .take(300)
    {
        let family = dir.file_name().to_string_lossy().into_owned();
        let Ok(snapshot) = capture(&family) else {
            continue;
        };
        if snapshot.entries.is_empty() {
            continue;
        }
        assert!(
            snapshot.container_existed && snapshot.is_valid(),
            "{family}"
        );
        assert_eq!(snapshot, capture(&family).unwrap(), "{family} not stable");
        let values = ApplicationDataManager::CreateForPackageFamily(&HSTRING::from(&family))
            .unwrap()
            .LocalSettings()
            .unwrap()
            .Values()
            .unwrap();
        assert_eq!(
            values.Size().unwrap() as usize,
            snapshot.entries.len(),
            "{family}"
        );
        for entry in &snapshot.entries {
            let key = String::from_utf16_lossy(&entry.key_utf16);
            let direct = values.Lookup(&HSTRING::from(&key)).unwrap();
            assert_eq!(entry, &super::entry(&HSTRING::from(&key), &direct).unwrap());
            // Case-insensitive lookups, like the container itself.
            assert!(snapshot.has_key(&key.to_uppercase()), "{family} {key}");
            if let RawPackageValue::Bool { value } = entry.value {
                assert_eq!(
                    snapshot.read(&key, Kind::Bool),
                    bt_core::legacy_store::Read::Value(bt_core::legacy_xml::Value::Bool(value))
                );
            }
        }
        checked += 1;
        if checked == 3 {
            break;
        }
    }
    assert!(
        checked > 0,
        "no installed package with LocalSettings values found"
    );
}
