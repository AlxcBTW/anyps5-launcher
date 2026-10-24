//! Fails the build when `data/update-keys.txt` cannot be parsed.
//!
//! The launcher embeds that list and treats an unreadable one as "signing required", so a typo
//! would ship a launcher that refuses every update. The parser is the launcher's own
//! (`anyplay_update_manifest::TrustedKeys::parse`), not a copy.

use std::path::PathBuf;

fn main() {
    let list = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets this"))
        .join("../../data/update-keys.txt");
    println!("cargo:rerun-if-changed={}", list.display());
    println!("cargo:rerun-if-changed=build.rs");
    let text = match std::fs::read_to_string(&list) {
        Ok(text) => text,
        Err(error) => fail(&format!("cannot read {}: {error}", list.display())),
    };
    if let Err(error) = anyplay_update_manifest::TrustedKeys::parse(&text) {
        fail(&format!(
            "data/update-keys.txt is not a valid key list: {error}"
        ));
    }
}

fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(1);
}
