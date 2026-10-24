//! Every key here is made by the test and thrown away with it.

use super::*;
use time::Duration;

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()
}

fn manifest(version: &str, expires_in: Duration) -> Manifest {
    Manifest {
        schema: SCHEMA,
        version: version.into(),
        published: format_time(now() - Duration::days(1)),
        expires: format_time(now() + expires_in),
        minimum_version: "0.1.0".into(),
        platforms: BTreeMap::from([(
            WINDOWS_X64.into(),
            Artifact {
                name: format!("AnyPlay-Launcher-{version}-x64.exe"),
                url: format!("https://example.test/AnyPlay-Launcher-{version}-x64.exe"),
                sha256: "ab".repeat(32),
                size: 1234,
            },
        )]),
    }
}

fn bytes(manifest: &Manifest) -> Vec<u8> {
    serde_json::to_vec_pretty(manifest).unwrap()
}

fn signed(manifest: &Manifest, key: &SigningKey) -> (Vec<u8>, String) {
    let bytes = bytes(manifest);
    let signature = sign(&bytes, key);
    (bytes, signature)
}

#[test]
fn a_valid_manifest_is_accepted() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let want = manifest("0.2.0", Duration::days(30));
    let (bytes, signature) = signed(&want, &key);
    let got = verify(&bytes, &signature, &keys, now()).unwrap();
    assert_eq!(got, want);
    assert_eq!(got.allows_update_from("0.1.3"), Ok(()));
    assert_eq!(got.artifact(WINDOWS_X64).unwrap().size, 1234);
}

#[test]
fn a_bad_signature_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (bytes, signature) = signed(&manifest("0.2.0", Duration::days(30)), &key);
    // One flipped digit.
    let mut flipped = signature.trim().to_owned();
    let first = if flipped.starts_with('0') { "1" } else { "0" };
    flipped.replace_range(0..1, first);
    assert_eq!(
        verify(&bytes, &flipped, &keys, now()),
        Err(ManifestError::Signature)
    );
    // A manifest changed after signing (a longer expiry, say).
    let tampered = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("0.2.0", "9.9.9")
        .into_bytes();
    assert_eq!(
        verify(&tampered, &signature, &keys, now()),
        Err(ManifestError::Signature)
    );
}

#[test]
fn a_missing_or_unreadable_signature_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (bytes, signature) = signed(&manifest("0.2.0", Duration::days(30)), &key);
    for bad in ["", "   \n", "zz", &signature[..100], "not a signature"] {
        assert_eq!(
            verify(&bytes, bad, &keys, now()),
            Err(ManifestError::SignatureFormat),
            "{bad:?}"
        );
    }
}

#[test]
fn a_signature_from_another_key_is_refused() {
    let trusted = generate_signing_key();
    let other = generate_signing_key();
    let keys = TrustedKeys::of(trusted.verifying_key());
    let (bytes, signature) = signed(&manifest("0.2.0", Duration::days(30)), &other);
    assert_eq!(
        verify(&bytes, &signature, &keys, now()),
        Err(ManifestError::Signature)
    );
    // No trusted key at all verifies nothing.
    assert_eq!(
        verify(&bytes, &signature, &TrustedKeys::default(), now()),
        Err(ManifestError::Signature)
    );
}

#[test]
fn a_signature_made_without_the_domain_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let bytes = bytes(&manifest("0.2.0", Duration::days(30)));
    let raw = to_hex(&key.sign(&bytes).to_bytes());
    assert_eq!(
        verify(&bytes, &raw, &keys, now()),
        Err(ManifestError::Signature)
    );
}

#[test]
fn an_expired_manifest_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let stale = manifest("0.2.0", Duration::days(30));
    let (bytes, signature) = signed(&stale, &key);
    // Valid up to the instant, void from it.
    let at = stale.expires_at().unwrap();
    assert!(verify(&bytes, &signature, &keys, at - Duration::seconds(1)).is_ok());
    assert!(matches!(
        verify(&bytes, &signature, &keys, at),
        Err(ManifestError::Expired(_))
    ));
    assert!(matches!(
        verify(&bytes, &signature, &keys, at + Duration::days(400)),
        Err(ManifestError::Expired(_))
    ));
}

#[test]
fn a_rollback_or_an_equal_version_is_refused() {
    let m = manifest("0.2.0", Duration::days(30));
    assert_eq!(m.allows_update_from("0.1.9"), Ok(()));
    for running in ["0.2.0", "0.2.1", "0.3.0", "1.0.0"] {
        assert!(
            matches!(
                m.allows_update_from(running),
                Err(ManifestError::NotNewer { .. })
            ),
            "{running}"
        );
    }
    // The release is newer than its own pre-release and not the other way round.
    assert_eq!(m.allows_update_from("0.2.0-rc.1"), Ok(()));
    assert!(m.allows_update_from("garbage").is_err());
}

#[test]
fn a_running_version_below_the_minimum_is_refused() {
    let mut m = manifest("0.5.0", Duration::days(30));
    m.minimum_version = "0.3.0".into();
    assert_eq!(m.allows_update_from("0.3.0"), Ok(()));
    assert_eq!(m.allows_update_from("0.4.2"), Ok(()));
    assert!(matches!(
        m.allows_update_from("0.2.9"),
        Err(ManifestError::RunningTooOld { .. })
    ));
}

#[test]
fn a_manifest_that_breaks_its_own_rules_is_refused_even_when_signed() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let good = manifest("0.2.0", Duration::days(30));
    let cases: Vec<Manifest> = vec![
        Manifest {
            schema: 2,
            ..good.clone()
        },
        Manifest {
            version: "latest".into(),
            ..good.clone()
        },
        Manifest {
            version: "v0.2.0".into(),
            ..good.clone()
        },
        Manifest {
            minimum_version: "0.3.0".into(),
            ..good.clone()
        },
        Manifest {
            expires: good.published.clone(),
            ..good.clone()
        },
        Manifest {
            expires: "tomorrow".into(),
            ..good.clone()
        },
        {
            let mut m = good.clone();
            m.platforms.get_mut(WINDOWS_X64).unwrap().sha256 = "AB".repeat(32);
            m
        },
        {
            let mut m = good.clone();
            m.platforms.get_mut(WINDOWS_X64).unwrap().size = 0;
            m
        },
    ];
    for case in cases {
        let (bytes, signature) = signed(&case, &key);
        assert!(
            matches!(
                verify(&bytes, &signature, &keys, now()),
                Err(ManifestError::Malformed(_))
            ),
            "{case:?}"
        );
    }
    // Not even JSON.
    let junk = b"not json".to_vec();
    let signature = sign(&junk, &key);
    assert!(matches!(
        verify(&junk, &signature, &keys, now()),
        Err(ManifestError::Malformed(_))
    ));
}

#[test]
fn the_next_key_can_be_shipped_before_the_current_one_retires() {
    let (old, next) = (generate_signing_key(), generate_signing_key());
    let both = TrustedKeys::of_many([old.verifying_key(), next.verifying_key()]);
    let only_next = TrustedKeys::of(next.verifying_key());
    let m = manifest("0.2.0", Duration::days(30));
    let (by_old, sig_old) = signed(&m, &old);
    let (by_next, sig_next) = signed(&m, &next);
    // While both are listed, either signs; after the old one is dropped only the new one does.
    assert!(verify(&by_old, &sig_old, &both, now()).is_ok());
    assert!(verify(&by_next, &sig_next, &both, now()).is_ok());
    assert!(verify(&by_next, &sig_next, &only_next, now()).is_ok());
    assert_eq!(
        verify(&by_old, &sig_old, &only_next, now()),
        Err(ManifestError::Signature)
    );
}

#[test]
fn the_key_list_is_read_strictly() {
    let a = generate_signing_key().verifying_key();
    let b = generate_signing_key().verifying_key();
    let text = format!(
        "# trusted update keys\n\n{}\n  {}  \n",
        key_line(&a, "2026 offline key"),
        key_line(&b, "")
    );
    let keys = TrustedKeys::parse(&text).unwrap();
    assert_eq!(keys.len(), 2);
    assert!(!keys.is_empty());
    // Comments only: signing is not switched on.
    assert!(TrustedKeys::parse("# nothing yet\n\n").unwrap().is_empty());
    assert!(TrustedKeys::parse("").unwrap().is_empty());
    // Everything else fails and never reads as "no key".
    for bad in [
        "ed25519",
        "ed25519 abcd",
        &format!("ed25519 {}zz", "ab".repeat(31)),
        &format!("rsa {}", "ab".repeat(32)),
        &format!("ed25519:{}", "ab".repeat(32)),
        &format!("{}\ngarbage", key_line(&a, "")),
    ] {
        assert!(
            matches!(TrustedKeys::parse(bad), Err(ManifestError::KeyList(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn seeds_round_trip_and_hex_is_strict() {
    let key = generate_signing_key();
    let again = signing_key_from_hex(&seed_hex(&key)).unwrap();
    assert_eq!(key.verifying_key(), again.verifying_key());
    assert!(signing_key_from_hex("abcd").is_none());
    assert!(from_hex("abc").is_none());
    assert!(from_hex("zz").is_none());
    assert_eq!(from_hex("00ff").unwrap(), vec![0, 255]);
}
