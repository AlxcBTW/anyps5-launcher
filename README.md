# AnyPS5 Launcher

A native Windows launcher for [AnyPS5](https://github.com/boykopovar/AnyPS5), laid out like the PS5 home screen.

This is a port of the macOS launcher from [utkuhalis/AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM). Upstream AnyPS5 is [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5).

<p align="center">
  <img src="docs/media/preview.webp" width="800" alt="The launcher fades in like a PS5, then moves between Minecraft Dungeons II, Sword of the Sea and Hollow Knight: Silksong">
</p>

<p align="center"><a href="https://github.com/AlxcBTW/anyps5-launcher/releases/download/v0.1.0/AnyPS5-Launcher-Update.mp4"><b>Watch the update video with sound</b></a></p>

| | |
|---|---|
| ![The home screen with Minecraft Dungeons II selected](docs/media/home.jpg) | ![The controls of Minecraft Dungeons II, read from the game's own files](docs/media/controls-from-the-game.jpg) |
| ![Sword of the Sea selected on the home screen](docs/media/sword-of-the-sea.jpg) | ![Hollow Knight: Silksong, not unlocked yet](docs/media/silksong.jpg) |

<sub>Game artwork belongs to its publishers and is shown only to illustrate the launcher.</sub>

## What it does

- Shows your games like the PS5 home screen, with each game's own artwork, a fade-in on start, soft system sounds and quiet home music.
- Unlocks and starts games with AnyPS5, and downloads the AnyPS5 toolkit by itself on the first start.
- Reads a game's controls from its own files (Unreal Engine games, including Enhanced Input): every button shows what it does in the game, and one choice gives you the game's own PC keys. Ready-made layouts for mouse and keyboard are there too.
- Runs fullscreen like a console, with a clean window when you want one (F11).
- Shows trophies, the running game on Discord, logs and per-game settings.

## Download

Get the latest version from the [releases page](https://github.com/AlxcBTW/anyps5-launcher/releases/latest). It needs 64-bit Windows 10 or 11. There is nothing else to install first.

- **`AnyPS5-Launcher-<version>-Setup.exe`** installs the launcher for your user account (no administrator rights). It adds a Start menu entry, an optional desktop shortcut and an uninstaller.
- **`AnyPS5-Launcher-<version>-x64.exe`** is the same launcher as a single file. Put it anywhere and run it.

Each release lists SHA-256 checksums in `SHA256SUMS.txt`. To check a download, run `Get-FileHash <file>` in PowerShell and compare.

### Windows says "Windows protected your PC"

The files are not code-signed yet, so SmartScreen warns about them. Click **More info**, then **Run anyway**. You only see this once per file.

## First start

1. Choose a language and a look.
2. Pick your games folder, the one that holds your decrypted game dumps. The launcher lists what it finds there.
3. That is all. On the first start the launcher downloads the AnyPS5 toolkit by itself and shows a message when it is ready. If the download fails (no internet, for example), open Settings and choose *Download the Toolkit*.
4. Select a game, unlock it and press play.

The launcher keeps its settings in `%APPDATA%\AnyPS5` and the toolkit in `%LOCALAPPDATA%\AnyPS5`. Uninstalling the launcher leaves both in place.

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

- **MSVC** (`x86_64-pc-windows-msvc`, needs the Visual Studio Build Tools): what CI builds and what releases ship. `.cargo/config.toml` links the C runtime statically, so the exe needs no Visual C++ redistributable.
- **GNU** (`x86_64-pc-windows-gnu`): works for local development without Visual Studio. The default rustc linker setup needs a MinGW gcc; if the linker fails, build with `RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld -C linker-flavor=ld.lld"` and make `libshlwapi.a` (from any MinGW-w64) available through an extra `-L native=<dir>`. Do not build in a path that contains an apostrophe: `windres` cannot embed the icon there.

A release is made by pushing a tag `v<version>` that matches the version in `Cargo.toml`. The [release workflow](.github/workflows/release.yml) builds the portable exe and the installer (`tools/installer/anyps5-launcher.iss`, Inno Setup) and publishes both with their checksums. Started by hand it only builds them and keeps them as workflow artifacts.

`crates/anyps5-core` holds all logic without UI; `crates/anyps5-launcher` is the egui/wgpu application.

## Credits and licence

Licensed under GPL-2.0 (see [LICENSE](LICENSE)).

The original design and logic come from the macOS launcher by [utkuhalis](https://github.com/utkuhalis) ([AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM), GPL-2.0). This project changes it as a Windows port: new implementation, Windows file dialogs and window chrome, and a free font in place of Apple's SF Pro, which cannot be redistributed.

Bundled fonts and icons (licence texts in `crates/anyps5-launcher/assets/fonts/`): [Inter](https://rsms.me/inter/) (SIL OFL 1.1), [JetBrains Mono](https://www.jetbrains.com/lp/mono/) (SIL OFL 1.1) and [Phosphor Icons](https://phosphoricons.com) (MIT).

## Legal note

- You need your own decrypted game dumps. No game files, keys or firmware are included in this repository, and none will be.
- AnyPS5 is not an emulator. It ports executables so they run natively. This launcher only manages and starts them.
- Not affiliated with or endorsed by Sony.
