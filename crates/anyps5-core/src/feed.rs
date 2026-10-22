//! Small documents from the network, kept on disk: the toolkit's release feed, release notes and
//! the recommended settings. One call, [`fetch`], hides the cache file, the ETag, the age check
//! and the fallback to the last copy when the network fails.
//!
//! Only the callers know the hosts: they build the URLs from fixed https bases, so nothing here
//! takes a URL from data. The HTTP agent is the toolkit's.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::settings::write_atomic;
use crate::toolkit::{ToolkitError, agent, user_agent};

/// Where a document came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The cache was young enough; nothing was asked.
    Cached,
    /// The server said "not modified" (a request with the stored ETag).
    Revalidated,
    /// A new copy was downloaded and stored.
    Downloaded,
    /// The request failed; this is the last stored copy.
    Stale,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub body: String,
    pub source: Source,
}

/// What to fetch and how long a stored copy counts as current.
pub struct Request<'a> {
    pub url: &'a str,
    /// File name inside the cache folder (also names the `.etag` file).
    pub name: &'a str,
    pub accept: &'a str,
    /// A stored copy younger than this is returned without asking. `ZERO` always asks (with the
    /// ETag, so an unchanged document costs one small answer).
    pub max_age: Duration,
    /// The largest body accepted.
    pub max_bytes: u64,
}

/// The document at `request.url`, from the cache folder `dir` when it is current, else from the
/// network. A failing network gives the last stored copy ([`Source::Stale`]) if there is one.
/// Blocking: call from a worker thread.
pub fn fetch(dir: &Path, request: &Request<'_>) -> Result<Document, ToolkitError> {
    let body_file = dir.join(request.name);
    let etag_file = dir.join(format!("{}.etag", request.name));
    let stored = fs::read_to_string(&body_file).ok();
    if let Some(body) = &stored
        && request.max_age > Duration::ZERO
        && age(&body_file).is_some_and(|age| age < request.max_age)
    {
        return Ok(Document {
            body: body.clone(),
            source: Source::Cached,
        });
    }
    match download(
        request,
        stored
            .is_some()
            .then(|| fs::read_to_string(&etag_file).ok())
            .flatten(),
    ) {
        Ok(Answer::NotModified) => {
            let body = stored.ok_or_else(|| ToolkitError::Network("empty answer".into()))?;
            // Rewriting resets the age: the copy was confirmed current just now.
            let _ = write_atomic(&body_file, body.as_bytes());
            Ok(Document {
                body,
                source: Source::Revalidated,
            })
        }
        Ok(Answer::Body { body, etag }) => {
            store(dir, &body_file, &etag_file, &body, etag.as_deref())?;
            Ok(Document {
                body,
                source: Source::Downloaded,
            })
        }
        Err(error) => match stored {
            Some(body) => Ok(Document {
                body,
                source: Source::Stale,
            }),
            None => Err(error),
        },
    }
}

fn age(file: &Path) -> Option<Duration> {
    let modified = fs::metadata(file).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

fn store(
    dir: &Path,
    body_file: &Path,
    etag_file: &Path,
    body: &str,
    etag: Option<&str>,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_atomic(body_file, body.as_bytes())?;
    match etag {
        Some(etag) => write_atomic(etag_file, etag.as_bytes()),
        None => fs::remove_file(etag_file).or_else(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        }),
    }
}

enum Answer {
    NotModified,
    Body { body: String, etag: Option<String> },
}

fn download(request: &Request<'_>, etag: Option<String>) -> Result<Answer, ToolkitError> {
    let mut call = agent(Some(Duration::from_secs(15)))
        .get(request.url)
        .header("User-Agent", user_agent())
        .header("Accept", request.accept);
    if let Some(etag) = etag.as_deref().filter(|e| !e.trim().is_empty()) {
        call = call.header("If-None-Match", etag.trim());
    }
    let mut response = call
        .call()
        .map_err(|e| ToolkitError::Network(e.to_string()))?;
    match response.status().as_u16() {
        200 => {}
        304 => return Ok(Answer::NotModified),
        403 | 429 => {
            return Err(ToolkitError::Network(
                "rate limited, try again later".into(),
            ));
        }
        404 => return Err(ToolkitError::NoRelease),
        other => {
            return Err(ToolkitError::Network(format!(
                "the server answered {other}"
            )));
        }
    }
    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = response
        .body_mut()
        .with_config()
        .limit(request.max_bytes)
        .read_to_string()
        .map_err(|e| ToolkitError::Network(e.to_string()))?;
    Ok(Answer::Body { body, etag })
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A local server that answers every GET with `body` and the ETag `"v1"`, and 304 when the
    /// request carries that ETag. Returns its base URL and the number of requests seen.
    pub fn serve(body: &'static str) -> (String, Arc<AtomicUsize>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", server.server_addr());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                counter.fetch_add(1, Ordering::SeqCst);
                let matched = request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("If-None-Match") && h.value == "\"v1\"");
                let etag = tiny_http::Header::from_bytes("ETag", "\"v1\"").unwrap();
                let response = if matched {
                    tiny_http::Response::from_string("").with_status_code(304)
                } else {
                    tiny_http::Response::from_string(body)
                };
                let _ = request.respond(response.with_header(etag));
            }
        });
        (base, hits)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::serve;
    use super::*;
    use std::sync::atomic::Ordering;

    fn request<'a>(url: &'a str, max_age: Duration) -> Request<'a> {
        Request {
            url,
            name: "doc.json",
            accept: "application/json",
            max_age,
            max_bytes: 1024,
        }
    }

    #[test]
    fn a_young_copy_is_not_asked_for_again() {
        let dir = tempfile::tempdir().unwrap();
        let (base, hits) = serve("{\"a\":1}");
        let url = format!("{base}/doc");
        let day = Duration::from_secs(86_400);
        let first = fetch(dir.path(), &request(&url, day)).unwrap();
        assert_eq!(
            (first.body.as_str(), first.source),
            ("{\"a\":1}", Source::Downloaded)
        );
        let second = fetch(dir.path(), &request(&url, day)).unwrap();
        assert_eq!(second.source, Source::Cached);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_old_copy_is_revalidated_with_the_etag() {
        let dir = tempfile::tempdir().unwrap();
        let (base, hits) = serve("hello");
        let url = format!("{base}/doc");
        fetch(dir.path(), &request(&url, Duration::ZERO)).unwrap();
        let again = fetch(dir.path(), &request(&url, Duration::ZERO)).unwrap();
        assert_eq!(
            (again.body.as_str(), again.source),
            ("hello", Source::Revalidated)
        );
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failing_network_gives_the_last_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (base, _) = serve("kept");
        fetch(dir.path(), &request(&format!("{base}/doc"), Duration::ZERO)).unwrap();
        let down = request("http://127.0.0.1:9/doc", Duration::ZERO);
        let stale = fetch(dir.path(), &down).unwrap();
        assert_eq!((stale.body.as_str(), stale.source), ("kept", Source::Stale));
        let nothing = tempfile::tempdir().unwrap();
        assert!(fetch(nothing.path(), &down).is_err());
    }

    #[test]
    fn a_body_over_the_limit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (base, _) = serve("0123456789");
        let url = format!("{base}/doc");
        let mut small = request(&url, Duration::ZERO);
        small.max_bytes = 4;
        assert!(fetch(dir.path(), &small).is_err());
        assert!(!dir.path().join("doc.json").exists());
    }
}
