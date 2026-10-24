//! The signed update flow. Every key is made by the test and dropped with it; nothing here knows
//! the real key.

use super::*;
use anyplay_update_manifest::{generate_signing_key, sign};
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;
use time::Duration as Span;

const PROGRAM: &[u8] = b"MZ-new-signed-launcher";

fn sha_hex(data: &[u8]) -> String {
    toolkit::hex(&Sha256::digest(data))
}

/// Serves fixed bodies on localhost until the returned flag is set.
fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<AtomicBool>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(AtomicOrdering::SeqCst) {
            let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let path = request.url().to_string();
            let _ = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => request.respond(tiny_http::Response::from_data(body.clone())),
                None => request.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    (base, stop)
}

fn manifest_for(version: &str, program: &[u8], base: &str) -> Manifest {
    let now = OffsetDateTime::now_utc();
    Manifest {
        schema: anyplay_update_manifest::SCHEMA,
        version: version.into(),
        published: anyplay_update_manifest::format_time(now - Span::days(1)),
        expires: anyplay_update_manifest::format_time(now + Span::days(29)),
        minimum_version: "0.1.0".into(),
        platforms: BTreeMap::from([(
            WINDOWS_X64.into(),
            Artifact {
                name: exe_name(version),
                url: format!("{base}/exe"),
                sha256: sha_hex(program),
                size: program.len() as u64,
            },
        )]),
    }
}

/// The JSON of `releases/latest`. `tag` is what the release claims, which a signed flow ignores.
fn release_json(tag: &str, base: &str, with_manifest: bool, with_sig: bool) -> String {
    let version = tag.trim_start_matches('v');
    let mut assets = vec![
        format!(
            r#"{{"name": "AnyPlay-Launcher-{version}-x64.exe", "size": 4, "browser_download_url": "{base}/exe"}}"#
        ),
        format!(
            r#"{{"name": "SHA256SUMS.txt", "size": 9, "browser_download_url": "{base}/sums"}}"#
        ),
    ];
    if with_manifest {
        assets.push(format!(
            r#"{{"name": "latest.json", "size": 9, "browser_download_url": "{base}/latest.json"}}"#
        ));
    }
    if with_sig {
        assets.push(format!(
            r#"{{"name": "latest.json.sig", "size": 9, "browser_download_url": "{base}/latest.json.sig"}}"#
        ));
    }
    format!(
        r#"{{"tag_name": "{tag}", "assets": [{}]}}"#,
        assets.join(",")
    )
}

/// A server with a signed release 0.2.0 whose manifest `edit` may change before it is signed
/// with `signer`. Returns the base address, the stop flag and the release JSON.
fn serve_signed(
    signer: &SigningKey,
    program: &[u8],
    edit: impl FnOnce(&mut Manifest),
) -> (String, Arc<AtomicBool>, String) {
    // The address is needed inside the manifest, so bind first.
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let mut manifest = manifest_for("0.2.0", program, &base);
    edit(&mut manifest);
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    let signature = sign(&bytes, signer);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let routes: Vec<(&str, Vec<u8>)> = vec![
        ("/latest.json", bytes),
        ("/latest.json.sig", signature.into_bytes()),
        ("/exe", program.to_vec()),
    ];
    std::thread::spawn(move || {
        while !flag.load(AtomicOrdering::SeqCst) {
            let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let path = request.url().to_string();
            let _ = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => request.respond(tiny_http::Response::from_data(body.clone())),
                None => request.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    // The release itself claims a tag that is not the manifest's: it must not matter.
    let json = release_json("v9.9.9", &base, true, true);
    (base, stop, json)
}

fn fixture() -> (tempfile::TempDir, AppPaths, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(root.path());
    let exe = root.path().join("app").join("AnyPlay-Launcher.exe");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"MZ-old-launcher").unwrap();
    (root, paths, exe)
}

fn signed_check(
    signer: &SigningKey,
    trusted: &TrustedKeys,
    current: &str,
    edit: impl FnOnce(&mut Manifest),
) -> (Result<Option<Release>, SelfUpdateError>, Arc<AtomicBool>) {
    let (base, stop, json) = serve_signed(signer, PROGRAM, edit);
    let result = parse_signed(
        &json,
        current,
        trusted,
        OffsetDateTime::now_utc(),
        &mut |url, max| {
            assert!(url.starts_with(&base));
            get_small(url, max)
        },
    );
    (result, stop)
}

#[test]
fn a_valid_manifest_is_accepted_and_the_tag_of_the_release_is_ignored() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |_| {});
    stop.store(true, AtomicOrdering::SeqCst);
    let release = result.unwrap().expect("an update");
    assert_eq!(release.tag, "v0.2.0", "the version is the manifest's");
    assert_eq!(release.version(), "0.2.0");
    assert_eq!(release.exe.sha256, Some(sha_hex(PROGRAM)));
    assert_eq!(release.exe.size, PROGRAM.len() as u64);
    assert_eq!(release.vouch, Vouch::Manifest);
}

#[test]
fn a_bad_signature_and_a_wrong_key_are_refused() {
    let key = generate_signing_key();
    let other = generate_signing_key();
    // Signed by a key the launcher does not trust.
    let (result, stop) = signed_check(
        &other,
        &TrustedKeys::of(key.verifying_key()),
        "0.1.3",
        |_| {},
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Signature))
        ),
        "{result:?}"
    );

    // A signature that is garbage.
    let (base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    let manifest = get_small(&format!("{base}/latest.json"), 1 << 16).unwrap();
    let result = parse_signed(
        &json,
        "0.1.3",
        &TrustedKeys::of(key.verifying_key()),
        OffsetDateTime::now_utc(),
        &mut |url, _| {
            Ok(if url.ends_with(".sig") {
                format!("{}\n", "ab".repeat(64)).into_bytes()
            } else {
                manifest.clone()
            })
        },
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Signature))
        ),
        "{result:?}"
    );
}

#[test]
fn an_expired_manifest_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |m| {
        let now = OffsetDateTime::now_utc();
        m.published = anyplay_update_manifest::format_time(now - Span::days(40));
        m.expires = anyplay_update_manifest::format_time(now - Span::days(10));
    });
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Expired(_)))
        ),
        "{result:?}"
    );
}

#[test]
fn a_rollback_and_an_equal_version_offer_nothing() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    for running in ["0.2.0", "0.3.0", "1.0.0"] {
        let (result, stop) = signed_check(&key, &keys, running, |_| {});
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(matches!(result, Ok(None)), "{running}: {result:?}");
    }
}

#[test]
fn a_running_version_below_the_minimum_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |m| m.minimum_version = "0.1.5".into());
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(
                ManifestError::RunningTooOld { .. }
            ))
        ),
        "{result:?}"
    );
}

#[test]
fn a_release_without_a_manifest_or_signature_is_refused_with_no_fallback() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // Everything the unsigned flow needs is there: the exe and SHA256SUMS.txt.
    for (manifest, sig) in [(false, false), (true, false), (false, true)] {
        let json = release_json("v0.2.0", "http://127.0.0.1:9", manifest, sig);
        assert!(
            parse_latest(&json, "0.1.3").unwrap().is_some(),
            "the unsigned flow would take this release"
        );
        let result = parse_signed(
            &json,
            "0.1.3",
            &keys,
            OffsetDateTime::now_utc(),
            &mut |_, _| panic!("nothing is fetched without both files"),
        );
        assert!(
            matches!(result, Err(SelfUpdateError::NoManifest)),
            "{result:?}"
        );
    }
}

#[test]
fn check_with_keys_requires_the_manifest_and_without_keys_runs_the_old_path() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // A release that only the unsigned flow understands.
    let json = release_json("v0.2.0", "http://127.0.0.1:9", false, false);
    let (base, stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let none = TrustedKeys::default();
    let old = check_with(&base, "o/r", "0.1.3", &none).unwrap().unwrap();
    assert!(matches!(old.vouch, Vouch::Checksums(_)));
    assert_eq!(old.tag, "v0.2.0");
    let signed = check_with(&base, "o/r", "0.1.3", &keys);
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(signed, Err(SelfUpdateError::NoManifest)),
        "{signed:?}"
    );
}

#[test]
fn a_signed_release_is_found_through_the_api_staged_and_found_again() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    // The API answers on a second server; the assets come from the first.
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    let staged = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
    assert_eq!(staged.version, "0.2.0");
    assert_eq!(fs::read(&staged.path).unwrap(), PROGRAM);
    assert!(verified_marker(&staged.path).is_file());
    assert_eq!(
        pending_with(&paths, "0.1.3", true).map(|s| s.version),
        Some("0.2.0".into())
    );
    // A second stage reuses the file without the network.
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    std::thread::sleep(Duration::from_millis(150));
    stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();

    // A staged file that changed afterwards is no longer offered, and the swap removes the marker.
    fs::write(&staged.path, b"MZ-something-else!!").unwrap();
    assert!(pending_with(&paths, "0.1.3", true).is_none());
    fs::write(&staged.path, PROGRAM).unwrap();
    let again = pending_with(&paths, "0.1.3", true).expect("restored file is vouched for again");
    again.swap_in(&exe).unwrap();
    assert!(!verified_marker(&staged.path).exists());
    assert_eq!(fs::read(&exe).unwrap(), PROGRAM);
}

#[test]
fn a_download_with_another_hash_is_refused_and_leaves_nothing() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // The manifest vouches for one file, the server serves another of the same length.
    let (base, stop, json) = serve_signed(&key, b"MZ-tampered-launcher", |m| {
        let artifact = m.platforms.get_mut(WINDOWS_X64).unwrap();
        artifact.sha256 = sha_hex(PROGRAM);
        artifact.size = b"MZ-tampered-launcher".len() as u64;
    });
    let _ = base;
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    assert!(matches!(result, Err(SelfUpdateError::Digest)), "{result:?}");
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
    assert!(pending_with(&paths, "0.1.3", true).is_none());
}

#[test]
fn a_download_with_another_size_is_refused_and_leaves_nothing() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // The hash is right, the size the manifest states is not.
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |m| {
        m.platforms.get_mut(WINDOWS_X64).unwrap().size += 1;
    });
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    assert!(matches!(result, Err(SelfUpdateError::Size)), "{result:?}");
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
}

#[test]
fn a_manifest_that_points_somewhere_unsafe_is_refused_even_when_signed() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    type Edit = Box<dyn FnOnce(&mut Manifest)>;
    let edits: Vec<Edit> = vec![
        Box::new(|m| m.platforms.get_mut(WINDOWS_X64).unwrap().name = "..\\evil.exe".into()),
        Box::new(|m| m.platforms.get_mut(WINDOWS_X64).unwrap().name = "other.exe".into()),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url = "http://example.com/exe".into()
        }),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url = "http://127.0.0.1.evil.test/exe".into()
        }),
        Box::new(|m| {
            m.platforms.remove(WINDOWS_X64);
        }),
    ];
    for edit in edits {
        let (result, stop) = signed_check(&key, &keys, "0.1.3", edit);
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(result.is_err(), "{result:?}");
    }
}

#[test]
fn what_an_unsigned_launcher_left_staged_is_not_offered_once_signing_is_required() {
    let (_root, paths, _) = fixture();
    let path = updates_dir(&paths).join(exe_name("0.2.0"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, PROGRAM).unwrap();
    assert!(pending_with(&paths, "0.1.3", false).is_some());
    assert!(pending_with(&paths, "0.1.3", true).is_none());
    // A marker that names another hash does not vouch for it either.
    fs::write(verified_marker(&path), sha_hex(b"other")).unwrap();
    assert!(pending_with(&paths, "0.1.3", true).is_none());
    fs::write(verified_marker(&path), sha_hex(PROGRAM)).unwrap();
    assert!(pending_with(&paths, "0.1.3", true).is_some());
}

#[test]
fn cleanup_removes_markers_with_their_exes() {
    let (_root, paths, exe) = fixture();
    let dir = updates_dir(&paths);
    fs::create_dir_all(&dir).unwrap();
    let old = dir.join(exe_name("0.1.1"));
    let newer = dir.join(exe_name("0.3.0"));
    for path in [&old, &newer] {
        fs::write(path, b"MZ").unwrap();
        fs::write(verified_marker(path), "x").unwrap();
    }
    fs::write(dir.join("AnyPlay-Launcher-0.4.0-x64.exe.verified"), "x").unwrap();
    cleanup(&exe, &paths, "0.2.0");
    assert!(!old.exists() && !verified_marker(&old).exists());
    assert!(newer.exists() && verified_marker(&newer).exists());
    assert!(!dir.join("AnyPlay-Launcher-0.4.0-x64.exe.verified").exists());
}

#[test]
fn the_key_list_in_the_repository_is_readable_and_decides_the_mode() {
    // A line that is not a comment or a key fails here, in CI, instead of reading as "no key".
    let keys = TrustedKeys::parse(EMBEDDED_KEYS).expect("data/update-keys.txt must parse");
    assert_eq!(signing_required(), !keys.is_empty());
    // And an unreadable list is never taken for an empty one.
    assert!(TrustedKeys::parse("ed25519 nonsense").is_err());
}
