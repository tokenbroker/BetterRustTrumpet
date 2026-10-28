//! "Export diagnostic report" (TroubleshootEarTrumpetText): ErrorReporter.
//! DisplayDiagnosticData and LocalDataExporter. The report is written to a
//! local staging folder the user reviews in Explorer, then zipped beside
//! it; its path goes to the clipboard. Nothing is uploaded.
//!
//! Every text is passed through [`Sanitizer`] (PathSanitizer): user folders
//! become `%TEMP%`, `%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%`, and any
//! other `X:\Users\name` becomes `X:\Users\%USERNAME%`. The ten newest
//! reports are kept.
use crate::core::Core;
use bt_app::{
    i18n::{self, tr, trf},
    log::{self, Stamp},
    win32::{diag, dialog, theme},
};
use bt_audio::Snapshot;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

const LINE: &str = "--------------------------------------------------------------------";
/// LocalDataExporter keeps this many zips and copies this many log files.
const KEEP_BUNDLES: usize = 10;
const LOG_FILES: usize = 10;

/// PathSanitizer: the user's folders, most specific first.
pub struct Sanitizer {
    folders: Vec<(String, &'static str)>,
}

impl Sanitizer {
    pub fn new(folders: &[(Option<String>, &'static str)]) -> Self {
        Self {
            folders: folders
                .iter()
                .filter_map(|(path, token)| {
                    let path = path.as_deref()?.trim_end_matches(['\\', '/']);
                    // Too short to be a real folder (a bare drive root).
                    (path.chars().count() >= 4).then(|| (path.to_owned(), *token))
                })
                .collect(),
        }
    }

    /// Temp is normally below LocalAppData, so it goes first.
    pub fn current() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self::new(&[
            (var("TEMP"), "%TEMP%"),
            (var("LOCALAPPDATA"), "%LOCALAPPDATA%"),
            (var("APPDATA"), "%APPDATA%"),
            (var("USERPROFILE"), "%USERPROFILE%"),
        ])
    }

    pub fn sanitize(&self, text: &str) -> String {
        let mut text = text.to_owned();
        for (path, token) in &self.folders {
            text = replace_ignore_case(&text, path, token);
        }
        mask_user_names(&text)
    }
}

/// Ordinal case-insensitive replacement (ASCII case folding, which is what
/// matters for drive letters and folder names).
fn replace_ignore_case(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() {
        return text.to_owned();
    }
    let lower = text.to_ascii_lowercase();
    let needle = old.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut start = 0;
    while let Some(found) = lower[start..].find(&needle) {
        let at = start + found;
        out.push_str(&text[start..at]);
        out.push_str(new);
        start = at + needle.len();
    }
    out.push_str(&text[start..]);
    out
}

/// The `([A-Za-z]:\\Users\\)([^\\\/\s"']+)` -> `$1%USERNAME%` regex.
fn mask_user_names(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let is_prefix = bytes[i].is_ascii_alphabetic()
            && rest.len() > 9
            && rest[1..]
                .get(..8)
                .is_some_and(|s| s.eq_ignore_ascii_case(":\\users\\"));
        if is_prefix {
            let name_len = rest[9..]
                .find(|c: char| c == '\\' || c == '/' || c.is_whitespace() || c == '"' || c == '\'')
                .unwrap_or(rest.len() - 9);
            if name_len > 0 {
                out.push_str(&rest[..9]);
                out.push_str("%USERNAME%");
                i += 9 + name_len;
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn yes_no(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// The Rust port's data folder, beside its settings.
fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("BetterTrumpet-Rust")
}

/// ErrorReporter.GetLogDirectory.
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// GetDiagnosticsDirectory: `diagnostics` beside the logs.
fn diagnostics_dir() -> PathBuf {
    log::directory()
        .and_then(|d| d.parent().map(Path::to_path_buf))
        .unwrap_or_else(data_dir)
        .join("diagnostics")
}

/// SnapshotData's sections, the live audio state and the recent trace.
fn summary(core: &Core, log_text: &str, reason: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "BetterTrumpet diagnostic snapshot");
    let _ = writeln!(out, "{LINE}");
    let stamp = Stamp::now();
    let _ = writeln!(out, "createdAt: {}", stamp.full());
    let _ = writeln!(out, "reason: {reason}");
    let _ = writeln!(out, "{LINE}");
    let [gdi, user, global_gdi, global_user] = diag::gui_resources();
    section(
        &mut out,
        "App",
        &[
            ("version", env!("CARGO_PKG_VERSION").to_owned()),
            ("port", "Rust".to_owned()),
            ("runtimeMinutes", (log::uptime().as_secs() / 60).to_string()),
            ("gdiObjects", gdi.to_string()),
            ("userObjects", user.to_string()),
            ("globalGdiObjects", global_gdi.to_string()),
            ("globalUserObjects", global_user.to_string()),
            ("handleCount", diag::handle_count().to_string()),
            (
                "releaseStage",
                if cfg!(debug_assertions) {
                    "development"
                } else {
                    "production"
                }
                .to_owned(),
            ),
        ],
    );
    section(
        &mut out,
        "Device",
        &[
            ("osVersionBuild", diag::build_label()),
            (
                "osArchitecture",
                if cfg!(target_pointer_width = "64")
                    || std::env::var_os("PROCESSOR_ARCHITEW6432").is_some()
                {
                    "64 bit"
                } else {
                    "32 bit"
                }
                .to_owned(),
            ),
            (
                "processorCount",
                format!(
                    "{} core(s)",
                    std::thread::available_parallelism().map_or(1, |n| n.get())
                ),
            ),
        ],
    );
    section(
        &mut out,
        "AppSettings",
        &[
            ("IsLightTheme", yes_no(theme::apps_light()).into()),
            ("IsSystemLightTheme", yes_no(theme::system_light()).into()),
            ("IsRTL", yes_no(i18n::is_rtl()).into()),
            (
                "IsTransparencyEnabled",
                yes_no(theme::transparency()).into(),
            ),
            ("UseAccentColor", yes_no(theme::accent_on_taskbar()).into()),
            ("IsHighContrast", yes_no(theme::high_contrast_on()).into()),
            ("HasIdentity", yes_no(false).into()),
            ("CurrentUICulture", i18n::culture().to_owned()),
        ],
    );
    section(
        &mut out,
        "LocalOnly",
        &[("systemDpi", diag::system_dpi().to_string())],
    );
    let _ = writeln!(out, "Windows audio snapshot");
    let _ = writeln!(out, "{LINE}");
    match core.audio.snapshot() {
        Ok(snapshot) => dump_audio(&mut out, &snapshot),
        Err(e) => {
            let _ = writeln!(out, "Section failed: {e}");
        }
    }
    let _ = writeln!(out, "{LINE}");
    let _ = writeln!(out, "Recent in-memory trace");
    let _ = writeln!(out, "{LINE}");
    let _ = writeln!(
        out,
        "{}",
        if log_text.trim().is_empty() {
            "(empty)"
        } else {
            log_text
        }
    );
    out
}

fn section(out: &mut String, title: &str, entries: &[(&str, String)]) {
    let _ = writeln!(out, "{title}");
    let _ = writeln!(out, "{LINE}");
    for (key, value) in entries {
        let _ = writeln!(out, "{key}: {value}");
    }
    let _ = writeln!(out, "{LINE}");
}

/// DumpDevice / DumpSession over the snapshot.
fn dump_audio(out: &mut String, snapshot: &Snapshot) {
    for device in &snapshot.devices {
        let _ = writeln!(
            out,
            "{}{} {}%{} Id: {}",
            if device.is_default {
                "(Default Device) "
            } else {
                ""
            },
            device.name,
            (device.volume * 100.0).round() as i32,
            if device.muted { " (Muted)" } else { "" },
            device.id
        );
        for app in &device.apps {
            let _ = writeln!(out, "{LINE}");
            let indent = if app.sessions.len() <= 1 { "  " } else { "| " };
            let kind = if app.is_system_sounds {
                "SystemSounds"
            } else {
                "Desktop"
            };
            for session in &app.sessions {
                let _ = writeln!(out, "{indent}{}", session.display_name);
                let _ = writeln!(
                    out,
                    "{indent}  ({kind}) {}%{} Key: {}",
                    (session.volume * 100.0).round() as i32,
                    if session.muted { " (Muted)" } else { "" },
                    app.key
                );
                let _ = writeln!(
                    out,
                    "{indent}  Executable: {} ProcessId: {}",
                    app.executable.as_deref().unwrap_or(""),
                    session.process_id
                );
                if let Some(routed) = &app.routed_device_id {
                    let _ = writeln!(out, "{indent}  Persisted Endpoint Id: {routed}");
                }
            }
        }
        let _ = writeln!(out, "{LINE}");
    }
}

/// CleanupOldBundles: the ten newest zips stay.
fn cleanup(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut zips: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("BetterTrumpet-diagnostics-") && name.ends_with(".zip")
        })
        .map(|e| {
            let created = e
                .metadata()
                .and_then(|m| m.created())
                .unwrap_or(std::time::UNIX_EPOCH);
            (created, e.path())
        })
        .collect();
    zips.sort_by_key(|z| std::cmp::Reverse(z.0));
    for (_, path) in zips.into_iter().skip(KEEP_BUNDLES) {
        if let Err(e) = std::fs::remove_file(&path) {
            log::line(format!("LocalDataExporter CleanupOldBundles failed: {e}"));
        }
    }
}

/// CreateStagingFolder: README, the summary and the newest logs, all
/// sanitized.
pub fn create_staging(core: &Core, reason: &str) -> std::io::Result<PathBuf> {
    let dir = diagnostics_dir();
    std::fs::create_dir_all(&dir)?;
    cleanup(&dir);
    let stamp = Stamp::now();
    let staging = dir.join(format!(
        "BetterTrumpet-diagnostics-staging-{}",
        stamp.file_part(true)
    ));
    std::fs::create_dir_all(&staging)?;
    let sanitizer = Sanitizer::current();
    let write = |name: &str, text: &str| -> std::io::Result<()> {
        let path = staging.join(name.replace('/', "\\"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // UTF-8 with a BOM, as File.WriteAllText(..., Encoding.UTF8).
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(sanitizer.sanitize(text).as_bytes());
        std::fs::write(path, bytes)
    };
    write(
        "README.txt",
        "BetterTrumpet diagnostic bundle\r\n\r\n\
         Attach this zip file when reporting an issue.\r\n\
         It can contain app names, device names, process IDs, Windows audio endpoint IDs, settings state, and recent BetterTrumpet logs.\r\n\
         User folder paths are replaced with %USERPROFILE% / %APPDATA% / %LOCALAPPDATA% / %TEMP%.\r\n\
         Review it before sharing if your audio setup contains sensitive names.\r\n",
    )?;
    write(
        "diagnostic-summary.txt",
        &summary(core, &log::recent(), reason),
    )?;
    let mut errors = String::new();
    match log::directory() {
        Some(logs) if logs.is_dir() => {
            let mut files = log::log_files(&logs);
            files.sort_by_key(|f| {
                std::cmp::Reverse(
                    std::fs::metadata(f)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH),
                )
            });
            for file in files.into_iter().take(LOG_FILES) {
                let name = file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                match std::fs::read(&file) {
                    Ok(bytes) => write(&format!("logs/{name}"), &String::from_utf8_lossy(&bytes))?,
                    Err(e) => {
                        let _ = writeln!(errors, "{}: {e}", file.display());
                    }
                }
            }
        }
        other => {
            let _ = writeln!(
                errors,
                "Log directory not found: {}",
                other.map_or("(null)".into(), |d| d.display().to_string())
            );
        }
    }
    if !errors.is_empty() {
        write("log-copy-errors.txt", &errors)?;
    }
    Ok(staging)
}

/// ZipDirectory: `...-staging-<time>` becomes `...-<time>.zip` beside it;
/// the folder is deleted afterwards.
pub fn zip_directory(staging: &Path) -> std::io::Result<PathBuf> {
    let name = staging
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?
        .to_string_lossy()
        .replace("-staging-", "-");
    let zip = staging.with_file_name(format!("{name}.zip"));
    let mut entries = vec![];
    collect(staging, staging, &mut entries)?;
    std::fs::write(&zip, zip::stored(&entries))?;
    if let Err(e) = std::fs::remove_dir_all(staging) {
        log::line(format!(
            "LocalDataExporter ZipDirectory cleanup failed: {e}"
        ));
    }
    Ok(zip)
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((name, std::fs::read(&path)?));
        }
    }
    Ok(())
}

/// DisplayDiagnosticData: confirm, stage and show the files, then zip them
/// on OK (or keep the folder on Cancel), copying the path either way.
pub fn export(owner: isize, core: &Core) {
    if !dialog::confirm(
        owner,
        tr("DiagnosticsExportConfirmTitle"),
        tr("DiagnosticsExportConfirmMessage"),
    ) {
        return;
    }
    log::flush();
    let result = create_staging(core, "manual-export").and_then(|staging| {
        diag::show_in_explorer(&staging);
        if !dialog::confirm(
            owner,
            tr("DiagnosticsExportReviewTitle"),
            tr("DiagnosticsExportReviewMessage"),
        ) {
            diag::copy_text(&staging.to_string_lossy());
            let shown = Sanitizer::current().sanitize(&staging.to_string_lossy());
            dialog::message(
                owner,
                tr("DiagnosticsExportSuccessTitle"),
                &trf("DiagnosticsExportStagingKeptMessage", &[("0", &shown)]),
                false,
            );
            return Ok(());
        }
        let zip = zip_directory(&staging)?;
        diag::copy_text(&zip.to_string_lossy());
        diag::show_in_explorer(&zip);
        let shown = Sanitizer::current().sanitize(&zip.to_string_lossy());
        dialog::message(
            owner,
            tr("DiagnosticsExportSuccessTitle"),
            &trf("DiagnosticsExportSuccessMessage", &[("0", &shown)]),
            false,
        );
        Ok(())
    });
    if let Err(e) = result {
        log::line(format!("ErrorReporter DisplayDiagnosticData failed: {e}"));
        dialog::message(
            owner,
            tr("DiagnosticsExportFailedTitle"),
            &trf("DiagnosticsExportFailedMessage", &[("0", &e.to_string())]),
            true,
        );
    }
}

/// A minimal zip writer: stored (uncompressed) entries, which keeps the
/// report readable by any unzip tool without a compression dependency.
mod zip {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    0xedb8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// The archive bytes for (name, content) entries; names are UTF-8.
    pub fn stored(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![];
        let mut central = vec![];
        for (name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let size = data.len() as u32;
            let local = |out: &mut Vec<u8>, signature: u32| {
                out.extend(signature.to_le_bytes());
            };
            // Local file header.
            local(&mut out, 0x0403_4b50);
            out.extend(20u16.to_le_bytes()); // version needed
            out.extend(0x0800u16.to_le_bytes()); // UTF-8 names
            out.extend(0u16.to_le_bytes()); // stored
            out.extend([0, 0, 0x21, 0]); // 1980-01-01 00:00
            out.extend(crc.to_le_bytes());
            out.extend(size.to_le_bytes());
            out.extend(size.to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(data);
            // Central directory record.
            local(&mut central, 0x0201_4b50);
            central.extend(20u16.to_le_bytes()); // version made by
            central.extend(20u16.to_le_bytes());
            central.extend(0x0800u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend([0, 0, 0x21, 0]);
            central.extend(crc.to_le_bytes());
            central.extend(size.to_le_bytes());
            central.extend(size.to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0; 8]); // extra, comment, disk, internal attrs
            central.extend(0u32.to_le_bytes()); // external attrs
            central.extend(offset.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        let count = entries.len() as u16;
        out.extend(&central);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0; 4]); // disk numbers
        out.extend(count.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn writes_a_stored_archive() {
            assert_eq!(crc32(b"IEND"), 0xae42_6082);
            let zip = stored(&[("a/b.txt".into(), b"hello".to_vec())]);
            assert!(zip.starts_with(&[0x50, 0x4b, 0x03, 0x04]));
            // Local header (30) + name (7) + data (5), then the central
            // record (46 + 7) and the end record (22).
            assert_eq!(zip.len(), 30 + 7 + 5 + 46 + 7 + 22);
            let end = &zip[zip.len() - 22..];
            assert_eq!(&end[..4], &[0x50, 0x4b, 0x05, 0x06]);
            assert_eq!(u32::from_le_bytes(end[16..20].try_into().unwrap()), 42);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zips_a_staging_folder_and_removes_it() {
        let root = tempfile::tempdir().unwrap();
        let staging = root
            .path()
            .join("BetterTrumpet-diagnostics-staging-20260101-000000-000");
        std::fs::create_dir_all(staging.join("logs")).unwrap();
        std::fs::write(staging.join("README.txt"), "readme").unwrap();
        std::fs::write(staging.join("logs").join("a.log"), "log").unwrap();
        let zip = zip_directory(&staging).unwrap();
        assert_eq!(
            zip.file_name().unwrap(),
            "BetterTrumpet-diagnostics-20260101-000000-000.zip"
        );
        assert!(!staging.exists());
        let bytes = std::fs::read(&zip).unwrap();
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(has(b"README.txt") && has(b"logs/a.log") && has(b"readme"));
    }

    #[test]
    fn sanitizes_user_folders_like_the_original() {
        let s = Sanitizer::new(&[
            (Some(r"C:\Users\Ada\AppData\Local\Temp\".into()), "%TEMP%"),
            (Some(r"C:\Users\Ada\AppData\Local".into()), "%LOCALAPPDATA%"),
            (Some(r"C:\Users\Ada\AppData\Roaming".into()), "%APPDATA%"),
            (Some(r"C:\Users\Ada".into()), "%USERPROFILE%"),
            (Some("C:".into()), "%TOO_SHORT%"),
        ]);
        assert_eq!(
            s.sanitize(r"log c:\users\ada\appdata\local\temp\x.txt"),
            r"log %TEMP%\x.txt"
        );
        assert_eq!(
            s.sanitize(r"C:\Users\Ada\AppData\Local\BetterTrumpet-Rust\logs"),
            r"%LOCALAPPDATA%\BetterTrumpet-Rust\logs"
        );
        assert_eq!(s.sanitize(r"C:\Users\Ada\Music"), r"%USERPROFILE%\Music");
        assert_eq!(
            s.sanitize(r#"other D:\Users\Bob\x and "E:\users\Eve""#),
            r#"other D:\Users\%USERNAME%\x and "E:\users\%USERNAME%""#
        );
        assert_eq!(s.sanitize("C: drive é"), "C: drive é");
    }
}
