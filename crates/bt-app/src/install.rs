//! Installing an update (D56), after the user agrees. Security first:
//!
//! - Each release carries `update.json` (the version and every file's size
//!   and SHA-256) and `update.json.sig`, its ECDSA P-256 signature made by
//!   the release workflow with a key only the project holds. The public
//!   half is compiled in here.
//! - Nothing is written unless the signature verifies, the manifest names
//!   this app, its version is the release's and newer than this build (no
//!   downgrades or replays), and it lists exactly the app's two files.
//! - Downloads come only from this repository's release links, and each
//!   file must match its signed size and hash before it is staged.
//! - The running files are renamed aside, not deleted, and put back if
//!   any step of the swap fails; the next launch removes them.
use crate::updates::{Release, Version};

/// The release workflow's signing key (packaging/update-signing.pub.pem):
/// the P-256 point's x and y.
const KEY_X: [u8; 32] = [
    0x60, 0x98, 0x4b, 0x36, 0x40, 0xaa, 0xdf, 0xd5, 0x2f, 0x59, 0xa9, 0xc9, 0x11, 0xc4, 0xcb, 0xb1,
    0xdd, 0x39, 0x01, 0x5c, 0x2a, 0xc8, 0xb4, 0x4e, 0x85, 0x7d, 0xbd, 0x9d, 0xaa, 0x04, 0xf0, 0xf6,
];
const KEY_Y: [u8; 32] = [
    0x0b, 0x3b, 0xaa, 0xe1, 0xfb, 0xd0, 0x59, 0xc6, 0x4d, 0xae, 0x53, 0xd7, 0x68, 0x4b, 0x03, 0xbc,
    0x29, 0x3e, 0x4a, 0x8d, 0x3a, 0x23, 0xe5, 0xda, 0x84, 0x78, 0xa9, 0x75, 0x5a, 0x4a, 0x2c, 0x98,
];

/// The only files an update may replace, next to the running exe.
pub const FILES: [&str; 2] = ["BetterTrumpet.exe", "bt.exe"];
const MANIFEST: &str = "update.json";
const SIGNATURE: &str = "update.json.sig";
const APP: &str = "BetterRustTrumpet";
const MAX_FILE: usize = 64 << 20;
const MAX_MANIFEST: usize = 64 << 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: usize,
    pub sha256: [u8; 32],
}

/// An ASN.1 DER ECDSA signature (what `openssl dgst -sign` writes) as the
/// 64-byte r and s that CNG verifies.
pub fn der_to_raw(der: &[u8]) -> Option<[u8; 64]> {
    fn integer(input: &[u8]) -> Option<(&[u8], &[u8])> {
        let (&tag, rest) = input.split_first()?;
        let (&len, rest) = rest.split_first()?;
        let len = usize::from(len);
        if tag != 0x02 || len == 0 || len > 33 || rest.len() < len {
            return None;
        }
        let (value, rest) = rest.split_at(len);
        // A leading zero only keeps a high bit from reading as negative.
        let value = match value {
            [0, tail @ ..] if len == 33 => tail,
            v if v[0] & 0x80 != 0 => return None,
            v => v,
        };
        (value.len() <= 32).then_some((value, rest))
    }
    let [0x30, len, body @ ..] = der else {
        return None;
    };
    if usize::from(*len) != body.len() {
        return None;
    }
    let (r, rest) = integer(body)?;
    let (s, rest) = integer(rest)?;
    if !rest.is_empty() {
        return None;
    }
    let mut out = [0u8; 64];
    out[32 - r.len()..32].copy_from_slice(r);
    out[64 - s.len()..].copy_from_slice(s);
    Some(out)
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Reads a manifest whose signature already verified, and checks it is
/// for this app, for `release`, newer than `current`, and lists exactly
/// the app's files.
pub fn read_manifest(json: &[u8], release: &str, current: &str) -> Result<Vec<Entry>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| "the update manifest is not valid".to_string())?;
    if value["app"].as_str() != Some(APP) {
        return Err("the update manifest is for another app".into());
    }
    let version = value["version"].as_str().unwrap_or_default();
    let (Some(signed), Some(tagged), Some(running)) = (
        Version::parse(version),
        Version::parse(release),
        Version::parse(current),
    ) else {
        return Err("the update's version is not valid".into());
    };
    if signed != tagged {
        return Err("the update manifest is for another release".into());
    }
    if signed <= running {
        return Err("the update is not newer than this version".into());
    }
    let mut entries = vec![];
    for file in value["files"].as_array().into_iter().flatten() {
        let name = file["name"].as_str().unwrap_or_default();
        let size = file["size"].as_u64().unwrap_or(0) as usize;
        let hash = file["sha256"].as_str().and_then(hex32);
        let (true, Some(sha256)) = (FILES.contains(&name), hash) else {
            return Err(format!("the update lists an unexpected file ({name})"));
        };
        if size == 0 || size > MAX_FILE || entries.iter().any(|e: &Entry| e.name == name) {
            return Err(format!("the update lists {name} wrongly"));
        }
        entries.push(Entry {
            name: name.into(),
            size,
            sha256,
        });
    }
    if entries.len() != FILES.len() {
        return Err("the update does not list the app's files".into());
    }
    Ok(entries)
}

/// The release's download link for `name`, if it is one of this
/// repository's.
fn asset<'a>(release: &'a Release, name: &str) -> Result<&'a str, String> {
    let prefix = format!("{}/releases/download/", crate::links::REPO);
    release
        .assets
        .iter()
        .find(|(n, url)| n == name && url.starts_with(&prefix))
        .map(|(_, url)| url.as_str())
        .ok_or_else(|| format!("the release has no {name}"))
}

/// The folder the app runs from.
pub fn install_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(Into::into)
}

/// Whether the app may replace its own files (not, say, in Program Files
/// without admin rights).
pub fn can_install() -> bool {
    let Some(dir) = install_dir() else {
        return false;
    };
    let probe = dir.join(".update-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok && dir.join(FILES[0]).exists()
}

/// Downloads, verifies and stages `release` as `<file>.new` beside the
/// app. Blocking: run it off the UI thread.
#[cfg(windows)]
pub fn prepare(release: &Release) -> Result<(), String> {
    use crate::win32::{crypto, http};
    let agent = format!("BetterRustTrumpet/{}", crate::updates::CURRENT);
    let fetch = |name: &str, max: usize| {
        http::get_url(asset(release, name)?, &agent, max)
            .ok_or_else(|| format!("{name} could not be downloaded"))
    };
    let manifest = fetch(MANIFEST, MAX_MANIFEST)?;
    let signature = fetch(SIGNATURE, 256)?;
    let digest = crypto::sha256(&manifest).ok_or("SHA-256 is unavailable")?;
    let valid = der_to_raw(&signature)
        .is_some_and(|raw| crypto::verify_p256(&KEY_X, &KEY_Y, &digest, &raw));
    if !valid {
        return Err("the update's signature does not verify".into());
    }
    let entries = read_manifest(&manifest, &release.version, crate::updates::CURRENT)?;
    let dir = install_dir().ok_or("the app's folder is unknown")?;
    let mut staged = vec![];
    let result = (|| {
        for entry in &entries {
            let bytes = fetch(&entry.name, MAX_FILE)?;
            if bytes.len() != entry.size || crypto::sha256(&bytes) != Some(entry.sha256) {
                return Err(format!("{} does not match the signed manifest", entry.name));
            }
            let path = dir.join(format!("{}.new", entry.name));
            std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", entry.name))?;
            staged.push(path);
        }
        Ok(())
    })();
    if result.is_err() {
        for path in staged {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

/// Swaps the staged files in: each current file is renamed to `.old`
/// (Windows allows that for a running exe) and its `.new` takes its name.
/// Any failure puts everything back.
pub fn apply() -> Result<(), String> {
    let dir = install_dir().ok_or("the app's folder is unknown")?;
    let mut done: Vec<&str> = vec![];
    let rollback = |done: &[&str]| {
        for name in done.iter().rev() {
            let file = dir.join(name);
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::rename(dir.join(format!("{name}.old")), &file);
        }
    };
    for name in FILES {
        let file = dir.join(name);
        let old = dir.join(format!("{name}.old"));
        let new = dir.join(format!("{name}.new"));
        let _ = std::fs::remove_file(&old);
        let step = (|| {
            if file.exists() {
                std::fs::rename(&file, &old)?;
            }
            if let Err(e) = std::fs::rename(&new, &file) {
                let _ = std::fs::rename(&old, &file);
                return Err(e);
            }
            Ok(())
        })();
        if let Err(e) = step {
            rollback(&done);
            discard();
            return Err(format!("{name}: {e}"));
        }
        done.push(name);
    }
    Ok(())
}

/// Removes staged files that were not applied.
pub fn discard() {
    if let Some(dir) = install_dir() {
        for name in FILES {
            let _ = std::fs::remove_file(dir.join(format!("{name}.new")));
        }
    }
}

/// At startup: the files a finished update left aside, and any staged
/// files of one that never finished.
pub fn cleanup() {
    if let Some(dir) = install_dir() {
        for name in FILES {
            let _ = std::fs::remove_file(dir.join(format!("{name}.old")));
        }
    }
    discard();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(app: &str, version: &str, files: &[(&str, u64)]) -> Vec<u8> {
        let files: Vec<_> = files
            .iter()
            .map(|(name, size)| {
                serde_json::json!({"name": name, "size": size, "sha256": "ab".repeat(32)})
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({"app": app, "version": version, "files": files}))
            .unwrap()
    }

    const BOTH: [(&str, u64); 2] = [("BetterTrumpet.exe", 10), ("bt.exe", 5)];

    #[test]
    fn a_good_manifest_lists_both_files() {
        let entries = read_manifest(
            &manifest(APP, "0.1.0-alpha.4", &BOTH),
            "0.1.0-alpha.4",
            "0.1.0-alpha.3",
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sha256, [0xab; 32]);
    }

    #[test]
    fn manifests_for_other_apps_releases_or_older_versions_are_refused() {
        let check = |json: Vec<u8>, release: &str, current: &str| {
            read_manifest(&json, release, current).is_err()
        };
        assert!(check(manifest("Other", "0.2.0", &BOTH), "0.2.0", "0.1.0"));
        // Signed for 0.1.5 but published as 0.2.0: a replay.
        assert!(check(manifest(APP, "0.1.5", &BOTH), "0.2.0", "0.1.0"));
        // Not newer: a downgrade.
        assert!(check(manifest(APP, "0.1.0", &BOTH), "0.1.0", "0.1.0"));
        assert!(check(manifest(APP, "0.0.9", &BOTH), "0.0.9", "0.1.0"));
    }

    #[test]
    fn manifests_with_unexpected_files_are_refused() {
        let check = |files: &[(&str, u64)]| {
            read_manifest(&manifest(APP, "0.2.0", files), "0.2.0", "0.1.0").is_err()
        };
        assert!(check(&[("BetterTrumpet.exe", 10)]));
        assert!(check(&[("BetterTrumpet.exe", 10), ("..\\evil.exe", 5)]));
        assert!(check(&[
            ("BetterTrumpet.exe", 10),
            ("bt.exe", 5),
            ("x.dll", 1)
        ]));
        assert!(check(&[
            ("BetterTrumpet.exe", 10),
            ("BetterTrumpet.exe", 10)
        ]));
        assert!(check(&[("BetterTrumpet.exe", 0), ("bt.exe", 5)]));
    }

    #[test]
    fn der_signatures_become_raw_r_and_s() {
        // r with a sign-padding zero (33 bytes), s short (31 bytes).
        let mut der = vec![0x30, 2 + 33 + 2 + 31, 0x02, 33, 0x00];
        der.extend([0x80; 32]);
        der.extend([0x02, 31]);
        der.extend([0x11; 31]);
        let raw = der_to_raw(&der).unwrap();
        assert_eq!(&raw[..32], &[0x80; 32]);
        assert_eq!(raw[32], 0);
        assert_eq!(&raw[33..], &[0x11; 31]);
        assert!(der_to_raw(&der[..der.len() - 1]).is_none());
        assert!(der_to_raw(b"not der").is_none());
    }

    /// The compiled-in key verifies a signature openssl made with the
    /// project's key (`cargo test -p bt-app -- --ignored signed_by_the_key`
    /// with BT_TEST_MANIFEST and BT_TEST_SIGNATURE set).
    #[test]
    #[ignore = "needs a signed sample"]
    fn signed_by_the_key() {
        let manifest = std::fs::read(std::env::var("BT_TEST_MANIFEST").unwrap()).unwrap();
        let sig = std::fs::read(std::env::var("BT_TEST_SIGNATURE").unwrap()).unwrap();
        let digest = crate::win32::crypto::sha256(&manifest).unwrap();
        let raw = der_to_raw(&sig).unwrap();
        assert!(crate::win32::crypto::verify_p256(
            &KEY_X, &KEY_Y, &digest, &raw
        ));
        let mut tampered = manifest.clone();
        tampered[0] ^= 1;
        let digest = crate::win32::crypto::sha256(&tampered).unwrap();
        assert!(!crate::win32::crypto::verify_p256(
            &KEY_X, &KEY_Y, &digest, &raw
        ));
    }
}
