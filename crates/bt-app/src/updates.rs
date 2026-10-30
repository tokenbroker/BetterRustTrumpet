//! The update check (D55): asks this project's GitHub releases whether a
//! newer version is out and tells; installing is `install` (D56), only
//! when the user agrees. The request carries the app's name and version
//! and nothing else.
use std::sync::Mutex;

/// This build's version (the release tag without its "v").
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// A newer release worth telling about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// "0.1.0-alpha.3".
    pub version: String,
    /// The release's page.
    pub url: String,
    /// The release's files: name and download link.
    pub assets: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unchecked,
    Checking,
    UpToDate,
    Available(Release),
    /// Downloading and verifying it after the user agreed (D56).
    Installing(Release),
    /// No answer (offline, GitHub unreachable, or an answer not understood).
    Failed,
}

/// A semantic version: numbers, then pre-release identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pre: Vec<Ident>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    // Numeric identifiers sort before alphanumeric ones (SemVer 11.4.3).
    Number(u64),
    Text(String),
}

impl Version {
    /// "v0.1.0-alpha.2" or "0.1.0"; build metadata ("+...") is ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches(['v', 'V']);
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.').map(|n| n.parse::<u64>().ok());
        let major = numbers.next()??;
        let minor = numbers.next()??;
        let patch = numbers.next()??;
        if numbers.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => vec![],
            Some(pre) => pre
                .split('.')
                .map(|p| {
                    if p.is_empty() {
                        None
                    } else if let Ok(n) = p.parse() {
                        Some(Ident::Number(n))
                    } else {
                        Some(Ident::Text(p.to_ascii_lowercase()))
                    }
                })
                .collect::<Option<_>>()?,
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            // A pre-release comes before its release.
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The original's UpdateChannel filter (UpdateNotifyChannel): 0 every
/// newer version, 1 minor and major, 2 major only, 3 none.
pub fn relevant(current: &Version, newer: &Version, channel: i32) -> bool {
    match channel {
        3 => false,
        2 => newer.major > current.major,
        1 => {
            newer.major > current.major
                || (newer.major == current.major && newer.minor > current.minor)
        }
        _ => true,
    }
}

/// Picks the newest relevant release from GitHub's release list. Drafts
/// never count; pre-releases (GitHub's flag or a version suffix such as
/// `-alpha.6`, since alphas are published as Latest) count only when this
/// build is one. `None` when the answer is not a release list.
pub fn pick(json: &[u8], current: &str, channel: i32) -> Option<Option<Release>> {
    let current = Version::parse(current)?;
    let list: serde_json::Value = serde_json::from_slice(json).ok()?;
    let best = list
        .as_array()?
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| current.is_prerelease() || !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let url = r["html_url"].as_str()?;
            // Only this project's release pages are ever opened.
            if !url.starts_with(&format!("{}/releases/", crate::links::REPO)) {
                return None;
            }
            let version = Version::parse(tag)?;
            if version.is_prerelease() && !current.is_prerelease() {
                return None;
            }
            Some((version, tag, url, r))
        })
        .filter(|(v, ..)| *v > current && relevant(&current, v, channel))
        .max_by(|a, b| a.0.cmp(&b.0));
    Some(best.map(|(_, tag, url, r)| {
        Release {
            version: tag.trim_start_matches(['v', 'V']).to_string(),
            url: url.to_string(),
            assets: r["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    Some((
                        a["name"].as_str()?.to_string(),
                        a["browser_download_url"].as_str()?.to_string(),
                    ))
                })
                .collect(),
        }
    }))
}

static RESULT: Mutex<Option<Status>> = Mutex::new(None);

/// Checks on a background thread, then calls `done` there; `take` has the
/// result.
#[cfg(windows)]
pub fn start(channel: i32, done: impl FnOnce() + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("update check".into())
        .spawn(move || {
            let status = check(channel);
            *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(status);
            done();
        });
    if spawned.is_err() {
        *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Status::Failed);
    }
}

/// The finished check's result, once.
pub fn take() -> Option<Status> {
    RESULT.lock().unwrap_or_else(|e| e.into_inner()).take()
}

#[cfg(windows)]
fn check(channel: i32) -> Status {
    let agent = format!("BetterRustTrumpet/{CURRENT}");
    let body = crate::win32::http::get(
        crate::links::API_HOST,
        crate::links::RELEASES_API,
        &agent,
        "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n",
        1 << 20,
    );
    match body.and_then(|b| pick(&b, CURRENT, channel)) {
        Some(Some(release)) => Status::Available(release),
        Some(None) => Status::UpToDate,
        None => Status::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_sort_as_semver() {
        assert!(v("0.1.0-alpha.10") > v("0.1.0-alpha.2"));
        assert!(v("0.1.0") > v("0.1.0-alpha.9"));
        assert!(v("0.1.0-beta") > v("0.1.0-alpha.9"));
        assert!(v("0.1.0-alpha.1") > v("0.1.0-alpha"));
        assert!(v("0.1.0-alpha.beta") > v("0.1.0-alpha.1"));
        assert!(v("v1.0.0") > v("0.9.9"));
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
        assert!(Version::parse("1.2").is_none());
        assert!(Version::parse("1.2.3-").is_none());
        assert!(Version::parse("latest").is_none());
    }

    #[test]
    fn channels_follow_the_original() {
        let current = v("3.0.0");
        assert!(relevant(&current, &v("3.0.1"), 0));
        assert!(!relevant(&current, &v("3.0.1"), 1));
        assert!(relevant(&current, &v("3.1.0"), 1));
        assert!(!relevant(&current, &v("3.1.0"), 2));
        assert!(relevant(&current, &v("4.0.0"), 2));
        assert!(!relevant(&current, &v("4.0.0"), 3));
    }

    fn list(releases: &[(&str, bool, bool)]) -> Vec<u8> {
        let items: Vec<_> = releases
            .iter()
            .map(|(tag, pre, draft)| {
                serde_json::json!({
                    "tag_name": tag,
                    "html_url": format!("{}/releases/tag/{tag}", crate::links::REPO),
                    "prerelease": pre,
                    "draft": draft,
                })
            })
            .collect();
        serde_json::to_vec(&items).unwrap()
    }

    #[test]
    fn picks_the_newest_relevant_release() {
        let json = list(&[
            ("v0.1.0-alpha.2", true, false),
            ("v0.1.0-alpha.10", true, false),
            ("v0.1.0-alpha.11", true, true),
            ("v0.1.0-alpha.3", true, false),
        ]);
        let found = pick(&json, "0.1.0-alpha.2", 0).unwrap().unwrap();
        assert_eq!(found.version, "0.1.0-alpha.10");
        assert!(found.url.ends_with("/v0.1.0-alpha.10"));
        assert_eq!(pick(&json, "0.1.0-alpha.10", 0), Some(None));
    }

    #[test]
    fn stable_builds_ignore_prereleases() {
        // Flagged on GitHub, or only by its version (published as Latest).
        let json = list(&[
            ("v0.2.0-alpha.1", true, false),
            ("v0.2.0-beta.1", false, false),
            ("v0.1.1", false, false),
        ]);
        let found = pick(&json, "0.1.0", 0).unwrap().unwrap();
        assert_eq!(found.version, "0.1.1");
        assert_eq!(pick(&json, "0.1.1", 0), Some(None));
        // A pre-release build is offered the newer pre-release.
        let found = pick(&json, "0.1.0-alpha.3", 0).unwrap().unwrap();
        assert_eq!(found.version, "0.2.0-beta.1");
    }

    /// The real request (`cargo test -p bt-app -- --ignored asks_github`).
    #[test]
    #[ignore = "network"]
    fn asks_github() {
        let status = check(0);
        println!("{status:?}");
        assert_ne!(status, Status::Failed);
        // The real list offers a newer release to an older build.
        let body = crate::win32::http::get(
            crate::links::API_HOST,
            crate::links::RELEASES_API,
            "BetterRustTrumpet/test",
            "Accept: application/vnd.github+json\r\n",
            1 << 20,
        )
        .unwrap();
        let found = pick(&body, "0.0.1-alpha.1", 0).unwrap().unwrap();
        println!("{found:?}");
    }

    #[test]
    fn foreign_pages_and_odd_answers_are_ignored() {
        let json = serde_json::to_vec(&serde_json::json!([{
            "tag_name": "v9.0.0",
            "html_url": "https://example.com/release",
            "prerelease": false,
            "draft": false,
        }]))
        .unwrap();
        assert_eq!(pick(&json, "0.1.0", 0), Some(None));
        assert_eq!(pick(b"{\"message\":\"rate limited\"}", "0.1.0", 0), None);
        assert_eq!(pick(b"not json", "0.1.0", 0), None);
    }
}
