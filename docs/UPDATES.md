# Signed updates

How the AnyPlay Launcher decides that an update is genuine, and what the operator does to switch that on and to keep it working. The code is `crates/anyps5-core/src/selfupdate.rs` (the client) and `crates/update-manifest` (the manifest format, the checks and the `update-sign` tool). The interface side of updating (toast, About, swap on exit) is in [DESIGN.md 9.5](DESIGN.md#95-the-launcher-updates-itself) and does not change.

## Why

Until a key is trusted, the launcher believes the latest GitHub release: it downloads the exe and checks it against `SHA256SUMS.txt` from the same release. Whoever can publish a release (a stolen token, a compromised workflow) can therefore run code on every player's PC. A signature over the exe alone would not be enough either:

- **Rollback.** An attacker serves an old, genuine, signed, vulnerable release as the latest.
- **Freeze.** An attacker keeps serving a stale answer so a fix never arrives.

A signed manifest that carries a version and an expiry, both checked by the client, closes both.

## The manifest

Every release carries two extra assets next to the binaries: `latest.json` and `latest.json.sig`.

```json
{
  "schema": 1,
  "version": "0.2.0",
  "published": "2026-11-01T12:00:00Z",
  "expires": "2026-12-01T12:00:00Z",
  "minimum_version": "0.1.0",
  "platforms": {
    "windows-x86_64": {
      "name": "AnyPlay-Launcher-0.2.0-x64.exe",
      "url": "https://github.com/anyplay-gg/launcher/releases/download/v0.2.0/AnyPlay-Launcher-0.2.0-x64.exe",
      "sha256": "<64 lower-case hex digits>",
      "size": 12345678
    }
  }
}
```

| Field | Meaning |
|---|---|
| `schema` | The format. A launcher reads exactly `1` and refuses anything else. |
| `version` | The version this manifest offers, plain semver without `v`. |
| `published`, `expires` | RFC 3339, UTC. After `expires` the manifest is void. The release workflow sets 30 days; the tool refuses more than 90. |
| `minimum_version` | The oldest running version that may update from this manifest. A launcher below it refuses (it needs an intermediate release or the installer). The workflow takes it from the repository variable `UPDATE_MINIMUM_VERSION`, default `0.1.0`. |
| `platforms` | Per platform the exe's `name`, `url`, `sha256` and `size`. Today only `windows-x86_64`. |

`latest.json.sig` is one line: the Ed25519 signature in hex over `anyplay-launcher-update-manifest-v1\n` followed by the exact bytes of `latest.json`. The prefix means a signature made for this purpose is good for nothing else. The bytes are verified before they are parsed; nothing is canonicalised.

## What the launcher checks

Once a key is trusted ([Migration](#migration)), an update is taken only when all of this holds, and a refusal only writes a line to the log:

1. The release has `latest.json` and `latest.json.sig`. The release's tag, its `SHA256SUMS.txt` and its other asset names are not used.
2. The signature is valid (strict Ed25519) for one of the trusted keys.
3. The manifest is well formed: schema `1`, plain versions, `minimum_version` not above `version`, `expires` after `published`, lower-case hex hash, a size above zero.
4. It has not expired.
5. `version` is newer than the running version (semver, pre-releases below their release). An equal or older version offers nothing: this is the rollback check.
6. The running version is not below `minimum_version`.
7. The exe's `name` is exactly `AnyPlay-Launcher-<version>-x64.exe` (or the old `AnyPS5-Launcher-<version>-x64.exe`), and its `url` is https.
8. The download's SHA-256 **and** size are the manifest's. The file gets its final name only after that, and a `.verified` marker holding the hash is written beside it.

A staged exe from an earlier run is offered again only if it has its marker and still hashes to it; what an unsigned launcher left in the updates folder is ignored.

Offline, the check fails with a network error and the launcher runs as it is. A refused update never touches the installed exe: the swap only ever sees a file that passed 8.

### What this does not stop

- A signed manifest stays valid until its `expires`. An attacker who can serve files can offer a **genuine older release that is still newer than what the player runs**, for up to 30 days after that older manifest was made. That is why the expiry is short; for a release that fixes a serious hole, publish it and, if needed, raise `UPDATE_MINIMUM_VERSION` so launchers below the fixed release stop accepting older steps.
- A player's clock set into the past defeats the expiry check on that PC.
- A launcher released before this change (up to the version that carries the first key) does not check any signature. It keeps updating from the unsigned path, which is why every release still carries `SHA256SUMS.txt` and `AnyPS5-Launcher-<v>-x64.exe`. Those launchers stay exposed until their players update once.
- The exe is not code-signed (Windows SmartScreen).

## Migration

`data/update-keys.txt` lists the trusted public keys, one per line as `ed25519 <64 hex digits> [label]`; blank lines and `#` comments are ignored. It is embedded into the launcher at build time.

| The file holds | The launcher |
|---|---|
| No key (as now) | Updates exactly as before: latest release, `SHA256SUMS.txt`. Signing is not switched on. |
| One key or more | Requires a valid signed manifest. **There is no fallback to the unsigned path**, not for a release without a manifest, not for a failing signature, not ever. |
| A line that is neither a comment nor a valid key | Fails the build (a test parses the file), and a launcher that somehow carries such a file refuses to update. A damaged list is never read as "no key". |

The rule lives in `selfupdate::signing_required` and `selfupdate::check_with`. The release workflow enforces the other half: with a trusted key but no `UPDATE_SIGNING_KEY` secret the release **fails** instead of publishing something no launcher would accept; with neither it publishes as before.

The first release that carries a key is the one that makes its own players safe. Players on older launchers update to it through the unsigned path one last time.

## The key ceremony (operator, by hand)

The real key is made by the operator, never by an agent or by CI. Do this on a machine you trust, ideally offline.

### 1. Make the key

Build the tool once (`cargo build --release -p anyplay-update-manifest --bin update-sign`), then:

```
update-sign keygen --secret-out update-signing.key --public-out update-keys.line --label "anyplay 2026"
```

- The secret (32 bytes in hex) goes to `update-signing.key`. It is never printed and an existing file is never overwritten.
- The public line goes to `update-keys.line` and to the terminal; that one is meant to be shared.

### 2. Store the secret

- Put the contents of `update-signing.key` into the password manager, together with the label and the date. That is the backup; losing it means a rotation ([below](#rotate-the-key)), not a disaster.
- Add the same text as the repository secret **`UPDATE_SIGNING_KEY`** (anyplay-gg/launcher, Settings, Secrets and variables, Actions). Prefer an environment with required reviewers if you want a second pair of eyes on every release.
- Then delete the file (and empty the trash) from the machine it was made on, or keep it only on an encrypted offline drive.

### 3. Publish the public key

Open a pull request that adds the line from `update-keys.line` to `data/update-keys.txt`. Do not merge it before the secret from step 2 is set: after the merge, any release built from `main` needs the secret.

### 4. Release

Tag the release as usual. The workflow signs the manifest with the secret, checks the signature against `data/update-keys.txt` (a mismatch fails the release) and attaches `latest.json` and `latest.json.sig`. Check that both are among the release's assets. Test once with a launcher built from the commit that carries the key, against that release, before announcing.

To try the signing without publishing, run the release workflow by hand (it keeps all files as workflow artifacts and publishes nothing); `latest.json` and `latest.json.sig` are among them. Locally:

```
update-sign manifest --version 0.2.0 --exe AnyPlay-Launcher-0.2.0-x64.exe --out latest.json
update-sign sign --manifest latest.json --out latest.json.sig --key-file update-signing.key
update-sign verify --manifest latest.json --sig latest.json.sig --keys data/update-keys.txt --current 0.1.3
```

### Rotate the key

A key must be replaced when it may have leaked, or on a schedule. Keys are rotated without a gap because the list holds several:

1. Make the new key (step 1) and store it (step 2) under a **new** secret name, for instance `UPDATE_SIGNING_KEY_NEXT`; do not replace the old one yet.
2. Open a pull request that **adds** the new line to `data/update-keys.txt` and keeps the old one. Release. Now launchers carry both keys and accept either.
3. Wait until the players have taken that release (watch the download counts; a launcher takes the update on its next start).
4. Set `UPDATE_SIGNING_KEY` to the new secret, release again, check that it verifies.
5. Open a pull request that **removes** the old line. Release. Launchers that carry the new list refuse anything the old key signs.
6. Delete the old secret from the password manager.

If the old key **leaked**, shorten the wait: anyone holding it can sign a manifest for any launcher that still trusts it until step 5 has been taken. Skip step 3 as far as you can, say so in the release notes, and consider raising `UPDATE_MINIMUM_VERSION` so older launchers must take the rotated release before anything else.

If the secret is **lost** (not leaked), nothing can be signed with the old key any more and every launcher still trusts only that key. Those players need the installer from the releases page. Keep the backup.

## The tool

`update-sign` (crate `anyplay-update-manifest`) does the four jobs of the ceremony and of the workflow:

| Command | Does |
|---|---|
| `keygen --secret-out <file> [--public-out <file>] [--label <text>]` | Makes a key pair. Prints only the public line. |
| `manifest --version <v> --exe <file> --out <file> [--minimum <v>] [--days 30] [--base-url <url>]` | Hashes the exe and writes the manifest. The exe must be named `AnyPlay-Launcher-<v>-x64.exe`. |
| `sign --manifest <file> --out <file> (--key-env <NAME> \| --key-file <file>)` | Signs, after checking that the manifest is well formed, and verifies the new signature before writing it. Never prints the key. |
| `verify --manifest <file> --sig <file> --keys <file> [--current <v>]` | What the launcher does, for a release candidate. |

Tests use throwaway keys made inside the test; the repository holds no private key and no real public key yet.
