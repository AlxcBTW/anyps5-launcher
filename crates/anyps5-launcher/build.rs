//! Embeds the application icon and the version information into the Windows executable (the Windows
//! host builds it; embed-resource is a Windows-only build dependency).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
        let source = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("manifest"))
            .join("assets/icon.ico");
        // Next to the script, so a source path with an apostrophe cannot break windres.
        let icon = out.join("icon.ico");
        std::fs::copy(&source, &icon).expect("icon must be copyable");
        let script = resource_script(&icon.to_string_lossy().replace('\\', "/"));
        let rc = out.join("launcher.rc");
        std::fs::write(&rc, script).expect("resource script must be writable");
        embed_resource::compile(&rc, embed_resource::NONE)
            .manifest_optional()
            .expect("icon resource must compile");
    }
}

#[cfg(windows)]
/// The icon plus a VERSIONINFO block (Explorer's Details tab, the installer's checks).
fn resource_script(icon: &str) -> String {
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let numbers: Vec<u16> = (0..4).map(|_| parts.next().unwrap_or(0)).collect();
    let comma = format!(
        "{},{},{},{}",
        numbers[0], numbers[1], numbers[2], numbers[3]
    );
    format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {comma}
PRODUCTVERSION {comma}
FILEOS 0x40004
FILETYPE 0x1
{{
    BLOCK "StringFileInfo"
    {{
        BLOCK "040904B0"
        {{
            VALUE "CompanyName", "AnyPS5 Launcher contributors"
            VALUE "FileDescription", "AnyPS5 Launcher"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "anyps5-launcher"
            VALUE "LegalCopyright", "GPL-2.0-only"
            VALUE "OriginalFilename", "anyps5-launcher.exe"
            VALUE "ProductName", "AnyPS5 Launcher"
            VALUE "ProductVersion", "{version}"
        }}
    }}
    BLOCK "VarFileInfo"
    {{
        VALUE "Translation", 0x409, 1200
    }}
}}
"#
    )
}
