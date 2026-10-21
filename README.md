# AnyPS5 Launcher

A native Windows launcher for [AnyPS5](https://github.com/boykopovar/AnyPS5), laid out like the PS5 home screen.

This is a port of the macOS launcher from [utkuhalis/AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM). Upstream AnyPS5 is [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5).

**Status: work in progress.** Nothing usable has been released yet.

## What it is

A game library and launcher front end for AnyPS5. It keeps the design and behaviour of the macOS launcher and runs on Windows. The implementation is written in Rust (eframe/egui on wgpu).

## Building

The Rust scaffold is not in the repository yet. Build instructions will be added with it (`cargo build --release`).

## Credits and licence

Licensed under GPL-2.0 (see [LICENSE](LICENSE)).

The original design and logic come from the macOS launcher by [utkuhalis](https://github.com/utkuhalis) ([AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM), GPL-2.0). This project changes it as a Windows port: new implementation, Windows file dialogs and window chrome, and a free font in place of Apple's SF Pro, which cannot be redistributed.

## Legal note

- You need your own decrypted game dumps. No game files, keys or firmware are included in this repository, and none will be.
- AnyPS5 is not an emulator. It ports executables so they run natively. This launcher only manages and starts them.
- Not affiliated with or endorsed by Sony.
