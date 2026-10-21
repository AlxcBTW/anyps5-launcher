# AnyPS5 Launcher

A native Windows launcher for [AnyPS5](https://github.com/boykopovar/AnyPS5), laid out like the PS5 home screen.

This is a port of the macOS launcher from [utkuhalis/AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM). Upstream AnyPS5 is [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5).

**Status: work in progress.** Nothing usable has been released yet.

## What it is

A game library and launcher front end for AnyPS5. It keeps the design and behaviour of the macOS launcher and runs on Windows. The implementation is written in Rust (eframe/egui on wgpu).

## Building

You need a stable Rust toolchain, 1.88 or newer ([rustup](https://rustup.rs)).

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

The executable is `target/release/anyps5-launcher.exe`.

Two toolchains are used:

- **MSVC** (`x86_64-pc-windows-msvc`, needs the Visual Studio Build Tools): what CI builds and what releases ship.
- **GNU** (`x86_64-pc-windows-gnu`): works for local development without Visual Studio. The default rustc linker setup needs a MinGW gcc; if the linker fails, build with `RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld -C linker-flavor=ld.lld"` and make `libshlwapi.a` (from any MinGW-w64) available through an extra `-L native=<dir>`. Do not build in a path that contains an apostrophe: `windres` cannot embed the icon there.

`crates/anyps5-core` holds all logic without UI; `crates/anyps5-launcher` is the egui/wgpu application.

## Credits and licence

Licensed under GPL-2.0 (see [LICENSE](LICENSE)).

The original design and logic come from the macOS launcher by [utkuhalis](https://github.com/utkuhalis) ([AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM), GPL-2.0). This project changes it as a Windows port: new implementation, Windows file dialogs and window chrome, and a free font in place of Apple's SF Pro, which cannot be redistributed.

Bundled fonts and icons (licence texts in `crates/anyps5-launcher/assets/fonts/`): [Inter](https://rsms.me/inter/) (SIL OFL 1.1), [JetBrains Mono](https://www.jetbrains.com/lp/mono/) (SIL OFL 1.1) and [Phosphor Icons](https://phosphoricons.com) (MIT).

## Legal note

- You need your own decrypted game dumps. No game files, keys or firmware are included in this repository, and none will be.
- AnyPS5 is not an emulator. It ports executables so they run natively. This launcher only manages and starts them.
- Not affiliated with or endorsed by Sony.
