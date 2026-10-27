use super::*;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use windows::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    System::Registry::{
        KEY_ALL_ACCESS, REG_CREATED_NEW_KEY, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
        RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
    },
};

const PREFIX: &str = "Software\\BetterTrumpetRustTests\\";
fn unique_path() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{PREFIX}{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
struct Fixture {
    key: Option<Key>,
    path: String,
}
impl Fixture {
    fn new() -> Self {
        let path = unique_path();
        let units: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        let mut disposition = Default::default();
        // SAFETY: test-only, uniquely generated key below the explicit test
        // prefix. NUL-terminated input and valid output pointers remain live.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(units.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                Some(&mut disposition),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        assert_eq!(
            disposition, REG_CREATED_NEW_KEY,
            "never reuse an existing test key"
        );
        Self {
            key: Some(Key(key)),
            path,
        }
    }
    fn set(&self, value: &RawRegistryValue) {
        let mut name = value.name_utf16.clone();
        name.push(0);
        // SAFETY: test fixture owns this writable handle. Input slices are
        // live for the call; raw byte data deliberately tests registry formats.
        let status = unsafe {
            RegSetValueExW(
                self.key.as_ref().unwrap().0,
                PCWSTR(name.as_ptr()),
                None,
                REG_VALUE_TYPE(value.kind),
                Some(&value.bytes),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.key.take();
        assert!(self.path.starts_with(PREFIX) && self.path.len() > PREFIX.len());
        let path: Vec<u16> = self.path.encode_utf16().chain(Some(0)).collect();
        // SAFETY: deletion is confined to the uniquely owned disposable key
        // verified above; no EarTrumpet/user settings path can enter this field.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr())) };
        assert_eq!(status, ERROR_SUCCESS);
    }
}
fn bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}
fn value(name: &str, kind: u32, bytes: Vec<u8>) -> RawRegistryValue {
    RawRegistryValue {
        name_utf16: name.encode_utf16().collect(),
        kind,
        bytes,
    }
}

#[test]
fn native_snapshot_retains_all_names_types_and_bytes_without_writing() {
    let fixture = Fixture::new();
    let mut expected = vec![
        value("", 1, bytes("default")),
        value("BoolXml", 1, bytes("<boolean>false</boolean>")),
        value("Unicode.\u{65e5}\u{672c}", 1, bytes("retained")),
        value("Expand", 2, bytes("%TEMP%\\literal")),
        value("DWORD", 4, 0xfedcba98u32.to_le_bytes().to_vec()),
        value("QWORD", 11, 0xfedcba9876543210u64.to_le_bytes().to_vec()),
        value("Multi", 7, bytes("one\0two\0")),
        value("None", 0, vec![0, 255, 1]),
        value("Binary", 3, (0..40_000).map(|n| (n % 251) as u8).collect()),
        value(&"long".repeat(400), 1, vec![b'a', 0, b'b']),
    ];
    for v in &expected {
        fixture.set(v);
    }
    expected.sort_by(|a, b| a.name_utf16.cmp(&b.name_utf16));
    let before = metadata(fixture.key.as_ref().unwrap()).unwrap();
    let snapshot = read_hkcu_values(&fixture.path).unwrap();
    assert!(snapshot.key_existed && snapshot.is_valid());
    assert_eq!(snapshot.values.len(), expected.len());
    for (actual, expected) in snapshot.values.iter().zip(&expected) {
        assert_eq!(actual.name_utf16, expected.name_utf16);
        assert_eq!(actual.kind, expected.kind);
        assert!(
            actual.bytes == expected.bytes,
            "value name length {}: data length {} versus {}, actual tail {:?}, expected tail {:?}",
            actual.name_utf16.len(),
            actual.bytes.len(),
            expected.bytes.len(),
            &actual.bytes[actual.bytes.len().saturating_sub(8)..],
            &expected.bytes[expected.bytes.len().saturating_sub(8)..]
        );
    }
    assert!(before == metadata(fixture.key.as_ref().unwrap()).unwrap());
    assert!(
        read_hkcu_values(&fixture.path).unwrap() == snapshot,
        "snapshot changed after read"
    );
    let read_only = open(&fixture.path).unwrap().unwrap();
    let name = [b'x' as u16, 0];
    // SAFETY: test-only access-right probe on our disposable key. Inputs and
    // handle are valid; KEY_QUERY_VALUE must deny this attempted mutation.
    let status = unsafe {
        RegSetValueExW(
            read_only.0,
            PCWSTR(name.as_ptr()),
            None,
            REG_VALUE_TYPE(3),
            Some(&[1]),
        )
    };
    assert_eq!(status, ERROR_ACCESS_DENIED);
    assert!(
        read_hkcu_values(&fixture.path).unwrap() == snapshot,
        "snapshot changed after read"
    );
}

#[test]
fn missing_keys_remain_missing_and_invalid_paths_fail() {
    let path = unique_path();
    assert_eq!(
        read_hkcu_values(&path).unwrap(),
        RegistrySnapshot::default()
    );
    assert!(open(&path).unwrap().is_none());
    assert!(read_hkcu_values("").is_err());
    assert!(read_hkcu_values("Software\\bad\0name").is_err());
}

#[test]
fn native_capture_persists_and_reloads_without_touching_the_registry_source() {
    let fixture = Fixture::new();
    let expected = value("Unknown.Addon", 1, vec![97, 0, 98]);
    fixture.set(&expected);
    let before = metadata(fixture.key.as_ref().unwrap()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let first = bt_storage::registry(directory.path(), || read_hkcu_values(&fixture.path)).unwrap();
    assert_eq!(first.outcome, bt_storage::Outcome::Created);
    assert_eq!(first.snapshot.values, vec![expected]);
    let second = bt_storage::registry(directory.path(), || {
        panic!("must bypass source after migration")
    })
    .unwrap();
    assert_eq!(second.outcome, bt_storage::Outcome::Existing);
    assert_eq!(second.snapshot, first.snapshot);
    assert_eq!(
        second.snapshot.read(
            Some("unknown.addon"),
            bt_core::registry_values::Request::Value(bt_core::legacy_xml::Kind::String),
            &RegistryEnvironment
        ),
        bt_core::registry_values::Read::StringUtf16(vec![97])
    );
    assert_eq!(
        second.snapshot.read(
            Some("unknown.addon"),
            bt_core::registry_values::Request::NullDefaultString,
            &RegistryEnvironment
        ),
        bt_core::registry_values::Read::Throws
    );
    assert!(before == metadata(fixture.key.as_ref().unwrap()).unwrap());
    assert_eq!(read_hkcu_values(&fixture.path).unwrap(), first.snapshot);
}

#[test]
fn native_registry_casing_matches_captured_dotnet_lookups() {
    use bt_core::registry_values::Environment;
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../bt-core/tests/fixtures/registry_projection_reference.json"
    ))
    .unwrap();
    for c in reference["lookups"].as_array().unwrap() {
        let stored: Vec<_> = c["input"]["stored"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .collect();
        for q in c["queries"].as_array().unwrap() {
            let query: Vec<_> = q["query"]
                .as_str()
                .unwrap_or_default()
                .encode_utf16()
                .take_while(|c| *c != 0)
                .collect();
            assert_eq!(
                RegistryEnvironment.names_equal(&stored, &query),
                q["hasKey"].as_bool().unwrap(),
                "{q}"
            );
        }
    }
}

#[test]
fn native_expansion_preserves_units_and_obeys_windows_string_boundaries() {
    use bt_core::registry_values::Environment;
    use std::os::windows::ffi::OsStrExt;
    for (input, expected) in [
        (vec![], vec![]),
        (vec![0xd800], vec![0xd800]),
        (
            "literal".encode_utf16().collect(),
            "literal".encode_utf16().collect(),
        ),
        ("a\0tail".encode_utf16().collect(), vec![97]),
    ] {
        assert_eq!(RegistryEnvironment.expand(&input).unwrap(), expected);
    }
    if let Some(root) = std::env::var_os("SystemRoot") {
        assert_eq!(
            RegistryEnvironment
                .expand(&"%SystemRoot%".encode_utf16().collect::<Vec<_>>())
                .unwrap(),
            root.encode_wide().collect::<Vec<_>>()
        );
    }
}
