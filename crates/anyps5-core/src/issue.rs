//! "Report to AnyPS5": a game's problem as a pre-filled upstream issue (docs/DESIGN.md 5.19).
//!
//! Upstream (`boykopovar/AnyPS5`) asks for bugs in the issue form `bug_report.yml`; GitHub fills
//! a form's fields from query parameters named after the field ids. [`prepare`] builds that link
//! from the same [`ReportInput`] the report file is made of. Nothing is sent: the player sees the
//! text, the browser opens the form, and the player presses GitHub's own submit button.
//!
//! GitHub fails on links beyond about 7,000 characters. A report that does not fit gets a short body (the
//! essentials and the last lines of the log); the whole report text goes to the clipboard for the
//! player to paste into the form.

use std::io;

use serde_json::Value;

use crate::report::{ReportInput, diagnostic_text};
use crate::settings::GameSettings;

/// The repository the form belongs to.
pub const UPSTREAM: &str = "boykopovar/AnyPS5";

/// The template of upstream's `.github/ISSUE_TEMPLATE` for a title that stops, crashes or renders
/// wrongly. Its field ids are the query parameters below.
pub const TEMPLATE: &str = "bug_report.yml";

/// Longest link the launcher opens. Measured against github.com: 6,576 characters pass, about
/// 7,000 give an error page and about 9,000 are refused.
pub const MAX_URL_BYTES: usize = 6_000;

/// Log lines sent when everything fits, and the characters they may take.
const LOG_LINES: usize = 30;
const LOG_CHARS: usize = 2_000;
/// A log line is cut here.
const LINE_CHARS: usize = 200;
/// The title GitHub shows in the list.
const TITLE_CHARS: usize = 120;

/// What the confirmation shows and what happens after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    /// The link to open in the browser.
    pub url: String,
    /// The text of the form, field by field, as the player will find it there.
    pub preview: String,
    /// The whole report, when the form could not take all of it: for the clipboard.
    pub clipboard: Option<String>,
}

/// One tier of detail: how much log and settings go into the link.
struct Tier {
    log_lines: usize,
    log_chars: usize,
    /// Fields are cut to a few hundred characters and the settings are left to the pasted report.
    short: bool,
}

const TIERS: [Tier; 4] = [
    Tier {
        log_lines: LOG_LINES,
        log_chars: LOG_CHARS,
        short: false,
    },
    Tier {
        log_lines: 8,
        log_chars: 800,
        short: true,
    },
    Tier {
        log_lines: 3,
        log_chars: 300,
        short: true,
    },
    Tier {
        log_lines: 0,
        log_chars: 0,
        short: true,
    },
];

/// Builds the link and the preview for a report.
pub fn prepare(input: &ReportInput) -> Prepared {
    let last = TIERS.len() - 1;
    let mut index = 0;
    loop {
        let form = form(input, &TIERS[index]);
        let url = url(&form);
        // The last tier is taken as it is; it cannot be too long.
        if url.len() <= MAX_URL_BYTES || index == last {
            return Prepared {
                preview: preview(&form),
                clipboard: (index > 0).then(|| redact(&diagnostic_text(input))),
                url,
            };
        }
        index += 1;
    }
}

/// The form's fields in order: (query parameter, text). Empty fields are left out.
fn form(input: &ReportInput, tier: &Tier) -> Vec<(&'static str, String)> {
    let cap = |text: String, limit: usize| {
        if tier.short { cut(&text, limit) } else { text }
    };
    let name = redact(&input.game_name);
    let reason = input.reason.as_deref().map(redact).unwrap_or_default();
    let mut title = format!("{name} ({}) {}", input.title_id, input.version);
    if !reason.is_empty() {
        title = format!("{title}: {}", cut(&reason, 70));
    }
    let toolkit = input.toolkit_version.as_deref().unwrap_or("none");
    let commit = format!(
        "AnyPS5 toolkit {toolkit}, launcher {}",
        input.launcher_version
    );
    let system = {
        let gpus = input.system.gpu_lines();
        let gpus = if gpus.is_empty() {
            "GPU unknown".to_string()
        } else {
            gpus.join("; ")
        };
        let windows = if input.system.windows.is_empty() {
            "Windows"
        } else {
            &input.system.windows
        };
        format!("{windows}, {gpus}")
    };
    let mut steps = format!("Started {name} from the AnyPlay Launcher.");
    let settings = non_default_settings(&input.settings);
    if !tier.short {
        if settings.is_empty() {
            steps.push_str("\nSettings: all at their defaults.");
        } else {
            steps.push_str("\nSettings that are not at their defaults:");
            for line in &settings {
                steps.push_str(&format!("\n- {line}"));
            }
        }
    }
    let mut notes = String::new();
    if tier.short {
        notes.push_str(
            "The launcher copied the full report (versions, PC, settings, log) to the clipboard. Paste it here.",
        );
    }
    let mut fields = vec![
        ("title", cut(&title, TITLE_CHARS)),
        ("summary", cap(reason, 200)),
        ("commit", cap(commit, 200)),
        ("system", cap(system, 300)),
        ("steps", cap(steps, 600)),
        (
            "log",
            log_tail(&input.session, tier.log_lines, tier.log_chars),
        ),
        ("notes", notes),
    ];
    fields.retain(|(_, text)| !text.trim().is_empty());
    fields
}

/// The link: the template, then the fields.
fn url(form: &[(&'static str, String)]) -> String {
    let mut url = format!("https://github.com/{UPSTREAM}/issues/new?template={TEMPLATE}");
    for (key, text) in form {
        url.push('&');
        url.push_str(key);
        url.push('=');
        url.push_str(&encode(text));
    }
    url
}

/// The form as the confirmation shows it: the label of each field of `bug_report.yml`, then its
/// text. `title` is the issue's title.
fn preview(form: &[(&'static str, String)]) -> String {
    let label = |key: &str| match key {
        "title" => "Title",
        "summary" => "What happens",
        "commit" => "AnyPS5 commit",
        "system" => "System",
        "steps" => "Steps to reproduce",
        "log" => "Log",
        _ => "What was checked",
    };
    form.iter()
        .map(|(key, text)| format!("{}\n{text}", label(key)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The last `lines` non-empty lines of the log, each cut, together at most `chars` characters;
/// the oldest lines go first.
pub fn log_tail(session: &str, lines: usize, chars: usize) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut total = 0;
    for line in session
        .lines()
        .rev()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .take(lines)
    {
        let line = cut(&redact(line), LINE_CHARS);
        let size = line.chars().count() + 1;
        if total + size > chars {
            break;
        }
        total += size;
        kept.push(line);
    }
    kept.reverse();
    kept.join("\n")
}

/// The settings that differ from a new game's, one `name: value` line each, in key order. The
/// keys are the JSON names the settings file uses; the custom keys count as one line.
pub fn non_default_settings(settings: &Value) -> Vec<String> {
    let defaults = serde_json::to_value(GameSettings::default()).unwrap_or(Value::Null);
    let (Some(now), Some(base)) = (settings.as_object(), defaults.as_object()) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for (key, value) in now {
        if base.get(key) == Some(value) {
            continue;
        }
        match (key.as_str(), value) {
            ("controls", Value::Object(changes)) => {
                if !changes.is_empty() {
                    lines.push(format!("controls: {} actions moved", changes.len()));
                }
            }
            ("tuning", Value::Object(tuning)) => {
                let default_tuning = serde_json::to_value(crate::controls::InputTuning::default())
                    .unwrap_or(Value::Null);
                for (name, value) in tuning {
                    if default_tuning.get(name) != Some(value) {
                        lines.push(format!("tuning.{name}: {value}"));
                    }
                }
            }
            _ => lines.push(format!("{key}: {value}")),
        }
    }
    lines
}

/// `C:\Users\alice\...` becomes `C:\Users\<user>\...` (either slash): the account name is nobody
/// else's business. Applied to everything that leaves in the link or the clipboard.
pub fn redact(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = ["\\users\\", "/users/"]
        .iter()
        .filter_map(|p| lower[at..].find(p).map(|i| (at + i, p.len())))
        .min()
    {
        let (start, length) = found;
        let name_start = start + length;
        let name_end = text[name_start..]
            .find(['\\', '/', '"', '\'', ' ', '\n', '\r', '\t'])
            .map_or(text.len(), |i| name_start + i);
        out.push_str(&text[at..name_start]);
        if name_end > name_start {
            out.push_str("<user>");
        }
        at = name_end;
    }
    out.push_str(&text[at..]);
    out
}

/// At most `limit` characters, with an ellipsis where something was cut.
fn cut(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Percent-encodes everything but the unreserved characters of RFC 3986.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Opens a GitHub link in the default browser. Anything but an `https://github.com/` link is
/// refused. No shell is involved.
pub fn open_url(url: &str) -> io::Result<()> {
    if !url.starts_with("https://github.com/") || url.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a github.com link",
        ));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(url)
            .spawn()?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::SystemInfo;

    fn input() -> ReportInput {
        ReportInput {
            game_name: "Minecraft Dungeons II".into(),
            title_id: "PPSA16064".into(),
            version: "01.100.100".into(),
            system: SystemInfo {
                cpu: "AMD Ryzen AI 9 365".into(),
                threads: 20,
                ram_gb: 32,
                gpus: vec!["NVIDIA GeForce RTX 5070 Laptop GPU".into()],
                drivers: vec!["581.57".into()],
                windows: "Windows 11 Home 25H2 (build 26200.1000)".into(),
            },
            launcher_version: "0.1.1".into(),
            toolkit_version: Some("0.1.3".into()),
            reason: Some("The GPU stopped responding.".into()),
            settings: serde_json::json!({
                "fullscreen": true,
                "vsync": true,
                "windowSize": 75,
                "overlay": "off",
                "gpu": "NVIDIA GeForce RTX 5070 Laptop GPU",
                "controls": {"Cross": ["KEY:F"], "Circle": []},
                "tuning": {"leftDeadzone": 12, "mouseDecayMs": 30}
            }),
            crash: None,
            session: "C:\\Users\\alice\\AppData\\x.log\nline 1\n\nline 2\nline 3".into(),
            now: "2026-10-22T09:31:00Z".into(),
        }
    }

    fn param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
        let query = url.split_once('?')?.1;
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
    }

    fn decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_link_fills_upstreams_bug_form() {
        let prepared = prepare(&input());
        assert!(prepared.url.starts_with(
            "https://github.com/boykopovar/AnyPS5/issues/new?template=bug_report.yml&"
        ));
        assert_eq!(prepared.clipboard, None);
        let get = |k: &str| decode(param(&prepared.url, k).unwrap());
        assert_eq!(
            get("title"),
            "Minecraft Dungeons II (PPSA16064) 01.100.100: The GPU stopped responding."
        );
        assert_eq!(get("summary"), "The GPU stopped responding.");
        assert_eq!(get("commit"), "AnyPS5 toolkit 0.1.3, launcher 0.1.1");
        assert_eq!(
            get("system"),
            "Windows 11 Home 25H2 (build 26200.1000), NVIDIA GeForce RTX 5070 Laptop GPU (driver 581.57)"
        );
        let steps = get("steps");
        assert!(steps.contains("- fullscreen: true"));
        assert!(steps.contains("- gpu: \"NVIDIA GeForce RTX 5070 Laptop GPU\""));
        assert!(steps.contains("- controls: 2 actions moved"));
        assert!(steps.contains("- tuning.leftDeadzone: 12"));
        assert!(!steps.contains("vsync") && !steps.contains("mouseDecayMs"));
        assert_eq!(
            get("log"),
            "C:\\Users\\<user>\\AppData\\x.log\nline 1\nline 2\nline 3"
        );
        assert!(param(&prepared.url, "notes").is_none());
        assert!(prepared.preview.starts_with("Title\nMinecraft Dungeons II"));
        assert!(prepared.preview.contains("\n\nLog\nC:\\Users\\<user>"));
    }

    #[test]
    fn nothing_but_unreserved_characters_stay_plain() {
        assert_eq!(encode("a b&c=d#é\n"), "a%20b%26c%3Dd%23%C3%A9%0A");
        assert_eq!(encode("AZaz09-_.~"), "AZaz09-_.~");
    }

    #[test]
    fn a_game_without_a_diagnosis_leaves_the_summary_to_the_player() {
        let mut i = input();
        i.reason = None;
        i.settings = serde_json::json!({"vsync": true});
        let prepared = prepare(&i);
        assert!(param(&prepared.url, "summary").is_none());
        assert_eq!(
            decode(param(&prepared.url, "title").unwrap()),
            "Minecraft Dungeons II (PPSA16064) 01.100.100"
        );
        assert!(decode(param(&prepared.url, "steps").unwrap()).contains("all at their defaults"));
    }

    #[test]
    fn the_log_tail_keeps_the_end_within_its_lines_and_characters() {
        let session: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        let tail = log_tail(&session, 3, 1000);
        assert_eq!(tail, "line 98\nline 99\nline 100");
        // 8 characters a line with its newline: three fit in 24, not four.
        assert_eq!(
            log_tail(&session, 40, 24).lines().count(),
            2,
            "line 100 is 9"
        );
        assert_eq!(log_tail("", 10, 100), "");
        let long = format!("{}\n", "x".repeat(500));
        assert_eq!(log_tail(&long, 5, 1000).chars().count(), LINE_CHARS);
        assert!(log_tail(&long, 5, 1000).ends_with('…'));
    }

    #[test]
    fn a_long_log_gets_a_short_body_and_the_whole_report_on_the_clipboard() {
        let mut i = input();
        i.session = (0..4000)
            .map(|n| {
                format!(
                    "2026-10-22T09:30:{:02}Z [vulkan] frame {n} {}\n",
                    n % 60,
                    "é".repeat(60)
                )
            })
            .collect();
        let prepared = prepare(&i);
        assert!(
            prepared.url.len() <= MAX_URL_BYTES,
            "{}",
            prepared.url.len()
        );
        let clipboard = prepared
            .clipboard
            .expect("the report goes to the clipboard");
        assert!(clipboard.starts_with("AnyPlay Launcher report, 2026-10-22T09:31:00Z"));
        assert!(clipboard.contains("frame 3999"));
        let notes = decode(param(&prepared.url, "notes").unwrap());
        assert!(notes.contains("Paste it here"));
        assert!(decode(param(&prepared.url, "log").unwrap()).lines().count() <= 8);
        assert!(
            prepared
                .preview
                .contains("What was checked\nThe launcher copied")
        );
    }

    #[test]
    fn the_link_never_exceeds_the_limit() {
        let mut i = input();
        i.game_name = "G".repeat(5_000);
        i.reason = Some("r".repeat(5_000));
        i.launcher_version = "v".repeat(5_000);
        i.system.windows = "w".repeat(5_000);
        i.session = "é".repeat(100_000);
        let prepared = prepare(&i);
        assert!(
            prepared.url.len() <= MAX_URL_BYTES,
            "{}",
            prepared.url.len()
        );
        assert!(prepared.clipboard.is_some());
    }

    #[test]
    fn the_user_name_never_leaves() {
        assert_eq!(
            redact("C:\\Users\\Alice Smith\\x D:/users/bob/y \"C:\\USERS\\carol\""),
            "C:\\Users\\<user> Smith\\x D:/users/<user>/y \"C:\\USERS\\<user>\""
        );
        assert_eq!(redact("no path"), "no path");
        assert_eq!(redact("C:\\Users\\"), "C:\\Users\\");
    }

    #[test]
    fn settings_at_their_defaults_say_nothing() {
        let defaults = serde_json::to_value(GameSettings::default()).unwrap();
        assert!(non_default_settings(&defaults).is_empty());
        assert!(non_default_settings(&Value::Null).is_empty());
        let moved = GameSettings {
            window_size: 50,
            ..Default::default()
        };
        assert_eq!(
            non_default_settings(&serde_json::to_value(moved).unwrap()),
            ["windowSize: 50"]
        );
    }

    #[test]
    fn only_github_links_open() {
        assert!(open_url("file:///C:/Windows/notepad.exe").is_err());
        assert!(open_url("https://example.com/").is_err());
        assert!(open_url("https://github.com/x y").is_err());
    }
}
