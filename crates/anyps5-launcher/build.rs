//! Embeds the application icon into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        embed_resource::compile("assets/icon.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("icon resource must compile");
    }
}
