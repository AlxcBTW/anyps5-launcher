//! Semantic versions, the way the launcher orders its releases.

use std::cmp::Ordering;

/// One dot-separated part of a pre-release tag (`rc`, `1`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Number(u64),
    Text(String),
}

impl Ord for Part {
    /// Numbers sort below text and compare as numbers; text compares as ASCII (semver 11.4).
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Part::Number(a), Part::Number(b)) => a.cmp(b),
            (Part::Number(_), Part::Text(_)) => Ordering::Less,
            (Part::Text(_), Part::Number(_)) => Ordering::Greater,
            (Part::Text(a), Part::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Part {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A semantic version: `major.minor.patch`, an optional `-pre.release` and ignored `+build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Part>,
}

impl Version {
    /// Reads `0.2.0`, `v0.2.0` or `0.2.0-rc.1+abc`; `None` for anything else.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split_once('+').map_or(text, |(rest, _)| rest);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.');
        let mut next = || -> Option<u64> {
            let part = numbers.next()?;
            let plain = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            // No leading zeros, like semver.
            (plain && (part == "0" || !part.starts_with('0')))
                .then(|| part.parse().ok())
                .flatten()
        };
        let core = [next()?, next()?, next()?];
        if numbers.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre
                .split('.')
                .map(|part| {
                    let valid = !part.is_empty()
                        && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                    if !valid {
                        None
                    } else if part.bytes().all(|b| b.is_ascii_digit()) {
                        part.parse().ok().map(Part::Number)
                    } else {
                        Some(Part::Text(part.to_owned()))
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Version { core, pre })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release is newer than its own pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Whether `latest` is a newer version than `current`; garbage is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}
