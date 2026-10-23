# AnyPS5 Launcher

A native launcher for [AnyPS5](https://github.com/boykopovar/AnyPS5) on Windows and Linux, laid out like the PS5 home screen.

This is a port of the macOS launcher from [utkuhalis/AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM). Upstream AnyPS5 is [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5).

<p align="center">
  <img src="docs/media/preview.webp" width="800" alt="The launcher fades in like a PS5, then moves between Minecraft Dungeons II, Sword of the Sea and Hollow Knight: Silksong">
</p>

<p align="center"><a href="https://github.com/anyplay-gg/launcher/releases/download/v0.1.0/AnyPS5-Launcher-Update.mp4"><b>Watch the update video with sound</b></a></p>

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
- Opens `anyps5://game/<title ID>` links from websites and messages, and adds "Open with AnyPS5" to folders; see [docs/DESIGN.md](docs/DESIGN.md) 9.5.

## Download

Get the latest version from the [releases page](https://github.com/anyplay-gg/launcher/releases/latest). On Windows it needs 64-bit Windows 10 or 11. There is nothing else to install first.

- **`AnyPS5-Launcher-<version>-Setup.exe`** installs the launcher for your user account (no administrator rights). It adds a Start menu entry, an optional desktop shortcut and an uninstaller.
- **`AnyPS5-Launcher-<version>-x64.exe`** is the same launcher as a single file. Put it anywhere and run it.

Each release lists SHA-256 checksums in `SHA256SUMS.txt`. To check a download, run `Get-FileHash <file>` in PowerShell and compare.

### Windows says "Windows protected your PC"

The files are not code-signed yet, so SmartScreen warns about them. Click **More info**, then **Run anyway**. You only see this once per file.

### Linux

The Linux build is new. It needs a 64-bit (x86-64) distribution with glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36, Arch and later), a Vulkan driver, and the ALSA and udev libraries that every desktop already has.

- **`AnyPS5-Launcher-<version>-x86_64.AppImage`**: make it executable and run it.

  ```sh
  chmod +x AnyPS5-Launcher-*-x86_64.AppImage
  ./AnyPS5-Launcher-*-x86_64.AppImage
  ```

  If your system has no FUSE 2, run it with `--appimage-extract-and-run`.
- **`AnyPS5-Launcher-<version>-linux-x86_64.tar.gz`** holds the plain executable, a `.desktop` file and the icon. Unpack it anywhere; copy the `.desktop` file to `~/.local/share/applications` and the icon to `~/.local/share/icons` for a menu entry.

The launcher keeps its settings in `~/.config/anyps5` and the toolkit, logs and caches in `~/.local/share/anyps5` (or where `XDG_CONFIG_HOME` and `XDG_DATA_HOME` point). It downloads the Linux toolkit (`relinker-<tag>` and `prx-linux-<tag>.zip`) from AnyPS5 itself.

What the games need from your system is up to AnyPS5, not the launcher:

- A CPU with AVX2, FMA, F16C, BMI1, BMI2, LZCNT and MOVBE (any Zen 2 or Intel Haswell and newer).
- A Vulkan 1.1 GPU. With Mesa drivers (AMD, Intel) the game imports memory through `/dev/udmabuf`: join the `kvm` group (`sudo usermod -aG kvm $USER`) and log in again. The launcher says so when a game fails for this reason.
- A window cannot be found or placed from outside on Wayland, so the launcher treats a game as shown after a few seconds, and the game's fullscreen and size follow its own settings (F11 in the game).

Self-update is not part of the Linux build yet; download a new version from the releases page.

## First start

1. Choose a language and a look.
2. Pick your games folder, the one that holds your decrypted game dumps. The launcher lists what it finds there.
3. That is all. On the first start the launcher downloads the AnyPS5 toolkit by itself and shows a message when it is ready. If the download fails (no internet, for example), open Settings and choose *Download the Toolkit*.
4. Select a game, unlock it and press play.

The launcher updates itself: it checks GitHub for a newer release when it starts, downloads and verifies it in the background, and installs it when you close the launcher (or at once with *Restart Now* in Settings, About). Switch *Check for Updates* off in Settings to stop that.

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

On Linux the executable is `target/release/anyps5-launcher`. Install the development packages first (Debian and Ubuntu names: `pkg-config libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev`). `tools/linux/package.sh <version> <binary> <folder>` makes the AppImage and the tarball.

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
