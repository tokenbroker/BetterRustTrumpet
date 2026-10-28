//! Local logging, after the original's ErrorReporter listeners: the last
//! 2000 lines in memory (CircularBufferTraceListener) for diagnostic
//! reports, and rotated files (FileTraceListener: `bettertrumpet-
//! yyyyMMdd-HHmmss.log`, a new file every 5 MB, at most 5 kept).
//!
//! Files stay local: they are only read back into a diagnostic report the
//! user creates. Writes are buffered and flushed on warnings and errors,
//! at most every two seconds otherwise, and on exit, so a line costs a
//! format and a copy. Nothing logs per frame.
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    thread::ThreadId,
    time::{Duration, Instant},
};

/// MAX_LOG_LINES.
const RING_LINES: usize = 2000;
/// MaxFileSizeBytes and MaxFileCount.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_FILES: usize = 5;
const BASE_NAME: &str = "bettertrumpet";
const FLUSH_EVERY: Duration = Duration::from_secs(2);

struct Logger {
    ring: VecDeque<String>,
    dir: Option<PathBuf>,
    file: Option<BufWriter<File>>,
    size: u64,
    /// The last flush; `None` before the first.
    flushed: Option<Instant>,
    ui_thread: Option<ThreadId>,
}

/// When logging started (App.Duration's start).
static STARTED: OnceLock<Instant> = OnceLock::new();

static LOGGER: Mutex<Logger> = Mutex::new(Logger {
    ring: VecDeque::new(),
    dir: None,
    file: None,
    size: 0,
    flushed: None,
    ui_thread: None,
});

/// A local wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
}

impl Stamp {
    /// From milliseconds since 0001-01-01 on the local clock (.NET
    /// `DateTime.Now` ticks / 10 000).
    pub fn from_local_ms(ms: i64) -> Self {
        let days = ms.div_euclid(86_400_000);
        let in_day = ms.rem_euclid(86_400_000);
        // 719 162 days from 0001-01-01 to 1970-01-01.
        let (year, month, day) = civil_from_days(days - 719_162);
        Self {
            year,
            month,
            day,
            hour: (in_day / 3_600_000) as u32,
            minute: (in_day / 60_000 % 60) as u32,
            second: (in_day / 1000 % 60) as u32,
            millis: (in_day % 1000) as u32,
        }
    }

    pub fn now() -> Self {
        #[cfg(windows)]
        let ms = crate::win32::clock::local_now_ms();
        #[cfg(not(windows))]
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
            + 62_135_596_800_000;
        Self::from_local_ms(ms)
    }

    /// `HH:mm:ss.fff`.
    pub fn time(&self) -> String {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            self.hour, self.minute, self.second, self.millis
        )
    }

    /// `yyyy-MM-dd HH:mm:ss.fff`.
    pub fn full(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {}",
            self.year,
            self.month,
            self.day,
            self.time()
        )
    }

    /// `yyyyMMdd-HHmmss`, plus `-fff` with `millis`.
    pub fn file_part(&self, millis: bool) -> String {
        let base = format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if millis {
            format!("{base}-{:03}", self.millis)
        } else {
            base
        }
    }
}

/// Days since 1970-01-01 to (year, month, day) (Howard Hinnant's
/// algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// FileTraceListener.DetectLevel: from the message's wording.
pub fn level(message: &str) -> &'static str {
    if message.contains("## FATAL") || message.contains("## UNHANDLED") {
        "FATAL"
    } else if message.contains("## WARNING")
        || message.contains("## Warning")
        || message.starts_with("WARN")
    {
        "WARN "
    } else if message.contains("ERROR")
        || message.contains("Exception")
        || message.contains("failed")
        || message.contains("Failed")
        || message.contains("panicked")
    {
        "ERROR"
    } else {
        "INFO "
    }
}

fn thread_label(ui: Option<ThreadId>) -> (String, String) {
    let current = std::thread::current().id();
    if Some(current) == ui {
        return ("UI".into(), "UI".into());
    }
    // "ThreadId(7)" -> 7.
    let id: String = format!("{current:?}")
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    (format!("{id:0>2}"), format!("{id:>2}  "))
}

fn open_file(dir: &Path, stamp: &Stamp) -> Option<(BufWriter<File>, u64)> {
    let path = dir.join(format!("{BASE_NAME}-{}.log", stamp.file_part(false)));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    let size = file.metadata().map_or(0, |m| m.len());
    Some((BufWriter::with_capacity(16 * 1024, file), size))
}

/// The log files in `dir`, oldest first (their names sort by time).
pub fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.starts_with(&format!("{BASE_NAME}-")) && n.ends_with(".log")
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

impl Logger {
    fn rotate(&mut self, stamp: &Stamp) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
        let mut files = log_files(&dir);
        while files.len() >= MAX_FILES {
            let _ = std::fs::remove_file(files.remove(0));
        }
        if let Some((file, size)) = open_file(&dir, stamp) {
            self.file = Some(file);
            self.size = size;
        }
    }

    fn line(&mut self, message: &str) {
        let stamp = Stamp::now();
        let (file_label, ring_label) = thread_label(self.ui_thread);
        self.ring
            .push_back(format!("{} {ring_label} {message}", stamp.time()));
        while self.ring.len() > RING_LINES {
            self.ring.pop_front();
        }
        let level = level(message);
        let Some(file) = &mut self.file else {
            return;
        };
        let formatted = format!("[{}] [{level}] [{file_label}] {message}\r\n", stamp.full());
        if file.write_all(formatted.as_bytes()).is_err() {
            return;
        }
        self.size += formatted.len() as u64;
        if level != "INFO " || self.flushed.is_none_or(|t| t.elapsed() >= FLUSH_EVERY) {
            let _ = file.flush();
            self.flushed = Some(Instant::now());
        }
        if self.size >= MAX_FILE_BYTES {
            self.rotate(&stamp);
        }
    }
}

/// Starts file logging in `dir` (created if missing); the calling thread
/// is labelled "UI". Without a call, lines only go to memory.
pub fn init(dir: &Path) {
    STARTED.get_or_init(Instant::now);
    let mut logger = LOGGER.lock().unwrap_or_else(|e| e.into_inner());
    logger.ui_thread = Some(std::thread::current().id());
    if std::fs::create_dir_all(dir).is_ok() {
        logger.dir = Some(dir.to_owned());
        if let Some((file, size)) = open_file(dir, &Stamp::now()) {
            logger.file = Some(file);
            logger.size = size;
        }
    }
}

/// Logs one line (`Trace.WriteLine`).
pub fn line(message: impl AsRef<str>) {
    let mut logger = LOGGER.lock().unwrap_or_else(|e| e.into_inner());
    logger.line(message.as_ref());
}

/// Writes buffered lines to the file (`Trace.Flush`).
pub fn flush() {
    let mut logger = LOGGER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(file) = &mut logger.file {
        let _ = file.flush();
    }
    logger.flushed = Some(Instant::now());
}

/// The in-memory lines (CircularBufferTraceListener.GetLogText).
pub fn recent() -> String {
    let logger = LOGGER.lock().unwrap_or_else(|e| e.into_inner());
    let mut text = String::new();
    for line in &logger.ring {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text
}

/// Time since [`init`] (zero before it).
pub fn uptime() -> Duration {
    STARTED.get().map_or(Duration::ZERO, Instant::elapsed)
}

/// The log directory, once file logging started.
pub fn directory() -> Option<PathBuf> {
    LOGGER.lock().unwrap_or_else(|e| e.into_inner()).dir.clone()
}

/// Logs panics as `## FATAL` lines and flushes, before the default hook
/// (CrashHandler's trace of unhandled exceptions).
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line(format!("## FATAL ## {info}"));
        flush();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_local_stamps() {
        // 2026-03-16 14:23:45.123 as .NET ticks / 10 000.
        let days_to_2026 = 739_690i64; // 0001-01-01 .. 2026-03-16
        let ms = days_to_2026 * 86_400_000 + ((14 * 60 + 23) * 60 + 45) * 1000 + 123;
        let stamp = Stamp::from_local_ms(ms);
        assert_eq!(stamp.full(), "2026-03-16 14:23:45.123");
        assert_eq!(stamp.file_part(false), "20260316-142345");
        assert_eq!(stamp.file_part(true), "20260316-142345-123");
        assert_eq!(Stamp::from_local_ms(0).full(), "0001-01-01 00:00:00.000");
    }

    #[test]
    fn detects_levels_like_the_original() {
        assert_eq!(level("## FATAL ## boom"), "FATAL");
        assert_eq!(level("WARN something"), "WARN ");
        assert_eq!(level("MediaSessionService: Init failed"), "ERROR");
        assert_eq!(level("Startup: tray icon visible"), "INFO ");
    }

    #[test]
    fn keeps_the_last_lines_in_memory() {
        line("first line for the ring test");
        assert!(recent().contains("first line for the ring test"));
    }
}
