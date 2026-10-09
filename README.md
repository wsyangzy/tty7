<div align="center">

<img src="assets/app-icon.svg" alt="tty7" width="88" height="88" />

### tty7

**A terminal workbench: persistent sessions, remote work, agents.**

<sub>Pure Rust · GPU rendering on Zed's gpui · VT core from Alacritty</sub>

<br />

[![CI](https://github.com/l0ng-ai/tty7/actions/workflows/ci.yml/badge.svg)](https://github.com/l0ng-ai/tty7/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/l0ng-ai/tty7?label=version&color=3FDD8C)](https://github.com/l0ng-ai/tty7/releases)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-blue)](https://github.com/l0ng-ai/tty7/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-join%20chat-5865F2?logo=discord&logoColor=white)](https://discord.gg/s3dethqz2V)

<sub>English · [简体中文](README.zh-CN.md)</sub>

<br />

<a href="assets/tour.mp4"><img src="assets/tour.webp" alt="One-minute tour of tty7: agents across repos, one agent driving another through the CLI, the prompt editor, diffs, dragging panes, and sessions that survive quitting the app" width="900" /></a>

<sub>▶ <a href="assets/tour.mp4">Watch in full quality</a></sub>

</div>

## Why

A background server owns your shells and panes — not the window. Everything
below follows from that.

- **Performance** — ~2× the throughput of Alacritty, Ghostty, or Kitty ([benchmarks](#benchmarks))
- **Persistent sessions** — quit the app and your shells keep running; after a reboot, layout, screen, and supported agent conversations come back. No tmux
- **Agent-aware** — Claude Code, Codex & co.: status, notifications, and git context for every repo at once
- **Scriptable by agents** — one agent opens a pane for another, hands off a task, waits, and reads the result, with or without the GUI running
- **Editor-grade input** — suggestions, completion, highlighting, history search, with no plugin to install
- **Remote development** — files, repos, panes, and git data stay on the remote machine, over a native SSH stack

## Install

Native builds for macOS, Windows, and Linux on [**Releases**](https://github.com/l0ng-ai/tty7/releases):

| | | |
|---|---|---|
| **macOS** | `…-macos-arm64.dmg` · `…-x86_64.dmg` | drag into Applications |
| **Windows** | `…-setup.exe` · portable `….zip` | |
| **Linux** | `…-x86_64.AppImage` | `chmod +x` and run — X11/Wayland libraries bundled |

For local builds, including the Android and iOS clients, see [Build from source](#build-from-source).

## What's inside

| | |
|---|---|
| **Agent-aware** | per-pane detection (28 CLIs) · status dot · notifications · branch + diff · tray icon when input is needed · resume after reboot · reopen any past session from Search Everywhere · one-key launch · tab sidebar grouped by repository, with pinned groups |
| **CLI + Skills** | bundled `tty7` CLI · [agent skill](skills/tty7/SKILL.md) · `run` streams a command and exits with its code · `split` · `send` · `wait --until free` · `capture` |
| **Editor-grade input** | ghost suggestions from history · explained tab completion · syntax highlighting · multi-line editing · click places the caret · <kbd>⌃ R</kbd> fuzzy history |
| **Window** | tabs & splits · <kbd>⌘ P</kbd> Search Everywhere · <kbd>⌘ F</kbd> scrollback search · hibernate a tab to free its memory · IME |
| **Side panel** | <kbd>⌘ J</kbd> · process tree and listening ports · file tree · changes and diffs · find in files · GitHub issues and pull requests, read-only, signed in through `gh` |
| **Shell integration** | injected when a pane starts, nothing to install · prompt marks · working directory · exit codes · command-finished notifications · zsh, bash, fish, PowerShell, WSL, remote panes |
| **Remote workspaces** | remote files, repos, changes, diffs, worktrees, tabs, and panes · reconnect from any client and continue where you left off |
| **SSH** | native russh stack: profiles with keychain secrets · SFTP panel · port forwarding · jump hosts · one-time, unprivileged `tty7-server` install |

## Supported agents

**Detection** is free: brand avatar, branch + diff, tab title.
**Status** takes one click under Settings → Integrations to install that agent's hook,
and brings the status dot, notifications, the tray icon, `tty7 wait`, and resume
after a reboot. **Fork** needs both — the agent's own fork command, and the hook
that tells tty7 which session to fork. **Past sessions** are read from the agent's
own history files and listed in Search Everywhere, where <kbd>⏎</kbd> resumes one.

<details>
<summary>The full support matrix, all twenty-eight</summary>

| Agent | Detected | Status · resume | Fork | Past sessions |
|---|:-:|:-:|:-:|:-:|
| **Claude Code** | ✓ | ✓ | ✓ | ✓ |
| **Codex** | ✓ | ✓ | ✓ | ✓ |
| **TraeCode** | ✓ | ✓ | ✓ |  |
| **Grok** | ✓ | ✓ | ✓ |  |
| **OpenCode** | ✓ | ✓ | ✓ | ✓ |
| **Oh My Pi** | ✓ | ✓ | ✓ | ✓ |
| **Prime Agent** | ✓ | ✓ | ✓ |  |
| **Droid** | ✓ | ✓ | ✓ | ✓ |
| **Qwen Code** | ✓ | ✓ | ✓ | ✓ |
| **Goose** | ✓ | ✓ | ✓ |  |
| **Qoder CLI** | ✓ | ✓ | ✓ | ✓ |
| **Qoder CN CLI** | ✓ | ✓ | ✓ | ✓ |
| **CodeBuddy** | ✓ | ✓ | ✓ | ✓ |
| **Gemini** | ✓ | ✓ | | ✓ |
| **Copilot** | ✓ | ✓ | | ✓ |
| **Kimi Code** | ✓ | ✓ | | ✓ |
| **Pi** | ✓ | ✓ | | ✓ |
| **Crush** | ✓ | ✓ | |  |
| **Antigravity** | ✓ | ✓ | |  |
| **Cursor** | ✓ | ✓ | | ✓ |
| **Empryo** | ✓ | ✓ | |  |
| **Muse Code** | ✓ | ✓ | |  |
| **jcode** | ✓ | ✓ | |  |
| Aider | ✓ | | |  |
| Amp | ✓ | | |  |
| Auggie | ✓ | | |  |
| Hermes | ✓ | | |  |
| Vibe | ✓ | | |  |

</details>

None of them are wrapped or proxied — the agent you start is the agent you get,
in a normal PTY, with its own interface. An agent launched through a wrapper
script can be mapped to one by name with `agent_commands` in `config.json`.
Every agent on your `PATH` is also a row in Search Everywhere (**Agent: Claude Code**, …)
that opens it in a new tab, and the New Tab menu names the ones you have actually
run. `agent_launch` sets the command line it starts with.

## Documentation

Full documentation lives in [**`docs/`**](docs/) —
[keyboard shortcuts](docs/reference/keyboard-shortcuts.mdx) ·
[config.json](docs/reference/configuration.mdx) ·
[CLI reference](docs/cli/reference.mdx). The agent-facing CLI interface is also
documented in [skills/tty7/SKILL.md](skills/tty7/SKILL.md).

Install the skill with:

```sh
npx skills add l0ng-ai/tty7    # install
npx skills update tty7         # update later
```

## Build from source

Install Git and the current stable Rust toolchain through `rustup`. Start each command block
at the repository root, on the operating system named below. Mobile commands then enter
`mobile/`, where the frontend dependencies live. Adding a Rust target does not install its
native compiler or SDK.

The desktop commands build `tty7-app`, the `tty7` CLI, `tty7-server`, and then `tty7-updater`.
Outputs are in `target/<target>/release/`, with `.exe` suffixes on Windows. These are the
compiled binaries; installer packaging is handled by the [GitHub Actions workflows](#github-actions).

### Windows

Use PowerShell with Visual Studio Build Tools' **Desktop development with C++**, a Windows
SDK, and CMake available. ARM64 also needs the MSVC ARM64 build tools and is an experimental
desktop release target.

```powershell
# Windows x64
rustup target add x86_64-pc-windows-msvc
cargo build --release --locked --target x86_64-pc-windows-msvc
cargo build --release --locked --features updater --bin tty7-updater --target x86_64-pc-windows-msvc

# Windows ARM64
rustup target add aarch64-pc-windows-msvc
cargo build --release --locked --target aarch64-pc-windows-msvc
cargo build --release --locked --features updater --bin tty7-updater --target aarch64-pc-windows-msvc
```

### macOS

Run in Bash or Zsh on macOS with Xcode and its command-line tools installed. Choose the
target for the Mac that will run the application.

```bash
# Apple Silicon
rustup target add aarch64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --features updater --bin tty7-updater --target aarch64-apple-darwin

# Intel
rustup target add x86_64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
cargo build --release --locked --features updater --bin tty7-updater --target x86_64-apple-darwin
```

### Linux desktop

The desktop release target is Linux x64. On Ubuntu or Debian, install the native build
dependencies below; other distributions need their equivalent packages.

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config cmake clang \
  libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev \
  libfreetype6-dev libwayland-dev libx11-dev libxcb1-dev \
  libzstd-dev libssl-dev libkrb5-dev

export LIBGSSAPI_IMPL=mit
rustup target add x86_64-unknown-linux-gnu
cargo build --release --locked --target x86_64-unknown-linux-gnu
cargo build --release --locked --features updater --bin tty7-updater --target x86_64-unknown-linux-gnu
```

### Standalone server

The desktop build already produces the matching `tty7-server`. To build only the server,
select its package explicitly. Windows x64 builds without the GUI also need NASM on `PATH`.
Use the Windows prerequisites above and run these commands in PowerShell:

```powershell
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
cargo build --release --locked -p tty7-server --target x86_64-pc-windows-msvc
cargo build --release --locked -p tty7-server --target aarch64-pc-windows-msvc
```

On macOS, with the Xcode prerequisites above:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked -p tty7-server --target aarch64-apple-darwin
cargo build --release --locked -p tty7-server --target x86_64-apple-darwin
```

For static Linux x64 and ARM64 servers, use a Linux build host with a C/C++ toolchain,
CMake, Zig **0.16.0** and `cargo-zigbuild` installed and on `PATH`, matching the workflows.
Keep `-p tty7-server`: the GUI's GSSAPI dependencies cannot be built for musl.

```bash
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo zigbuild --release --locked -p tty7-server --target x86_64-unknown-linux-musl
cargo zigbuild --release --locked -p tty7-server --target aarch64-unknown-linux-musl
```

Outputs are in `target/<target>/release/`. Linux ARM64 is a server target; the desktop
release matrix currently includes Linux x64 only.

### Standalone mobile gateway

The GUI embeds the phone gateway: enable **Settings → Mobile → Allow phone access**.
A standalone `tty7-server` instead needs `tty7-gateway` beside it or on `PATH` to serve
phones. The default desktop build does not emit a separate gateway executable.

On Windows, select the GUI package alongside the gateway to reuse its dependency features,
including `aws-lc-sys/prebuilt-nasm` for x64. Use the same Windows prerequisites as above:

```powershell
# Windows x64
rustup target add x86_64-pc-windows-msvc
cargo build --release --locked -p tty7 -p tty7-gateway --target x86_64-pc-windows-msvc

# Windows ARM64; requires the ARM64 target and MSVC build tools
rustup target add aarch64-pc-windows-msvc
cargo build --release --locked -p tty7 -p tty7-gateway --target aarch64-pc-windows-msvc
```

On macOS or Linux, with the platform's native build tools, build for the host architecture:

```bash
cargo build --release --locked -p tty7-gateway
```

The Windows gateway is in `target/<target>/release/tty7-gateway.exe`; the native macOS/Linux
command writes `target/release/tty7-gateway`.

### Android

The phone client lives in [`mobile/`](mobile/README.md). Use a current Node.js 22 release,
JDK **17–21**, the Android SDK and NDK, with `JAVA_HOME`, `ANDROID_HOME` and `NDK_HOME` set.
The mobile workflow pins NDK **28.2.13676358**. The commands below use PowerShell on Windows;
the same commands also work on a configured macOS or Linux host.

The Android native project is checked in with keyboard handling changes. **Do not run
`android init` over it.** Start at the repository root:

```powershell
cd mobile
rustup target add aarch64-linux-android
npm ci

# Installable ARM64 debug APK, signed with the local debug key
npx tauri android build --debug --apk --target aarch64

# ARM64 release APK; configure keystore.properties first for a signed APK
npx tauri android build --apk --target aarch64
```

APKs are written under `mobile/src-tauri/gen/android/app/build/outputs/apk/`.
For release signing, create the Git-ignored
`mobile/src-tauri/gen/android/keystore.properties` with your own values:

```properties
storeFile=C:/keys/tty7-release.jks
storePassword=<store-password>
keyAlias=<key-alias>
keyPassword=<key-password>
```

Use the absolute keystore path for your machine. Without this file, a release APK is
unsigned; the debug APK remains installable without release signing configuration.

### iOS

Build on **macOS with full Xcode** and an Apple developer account authorized for your
signing team. The workflow uses Xcode 26. For a fork, set your own `identifier` and
`bundle.iOS.developmentTeam` in [`tauri.conf.json`](mobile/src-tauri/tauri.conf.json), and
the matching `TEAM` in [`scripts/testflight.sh`](mobile/scripts/testflight.sh).

Sign into that team in Xcode, or set `ASC_KEY_ID`, `ASC_ISSUER_ID` and `ASC_KEY_PATH` for
an App Store Connect API key. Start at the repository root:

```bash
cd mobile
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
npm ci

# Archive and export an App Store-signed IPA without uploading to TestFlight
bash scripts/testflight.sh --no-upload
```

The script initializes the iOS project if it is missing and exports the IPA to
`mobile/build/testflight/`. Omitting `--no-upload` uploads the build to TestFlight.
See the [mobile README](mobile/README.md) for device and simulator development commands.

### GitHub Actions

The workflows cover the desktop, server and mobile targets above. GitHub uses the workflow
at the pushed ref; local edits and commits only become available there after they are pushed.

| Workflow | Trigger | Result |
|---|---|---|
| [`ci.yml`](.github/workflows/ci.yml) | Push to `main`, pull request, or manual run | Build and validation jobs |
| [`release.yml`](.github/workflows/release.yml) | `v*` tag or manual run | Desktop packages and standalone server artifacts; tag runs also assemble a draft release |
| [`nightly.yml`](.github/workflows/nightly.yml) | Daily at 18:00 UTC / 02:00 UTC+8, or manual run | When new commits are available, builds and updates the rolling `nightly` prerelease |
| [`mobile.yml`](.github/workflows/mobile.yml) | `mobile-v*` tag or manual run | Signed Android ARM64 APK artifact and an iOS build; tag runs also attach the APK to a draft release and upload iOS to TestFlight |

For a personal desktop package, run **Release** manually on your pushed branch and download
the appropriate artifact, such as `release-windows-x86_64`. A manual branch run does not
create a stable release. Mobile manual runs build both platforms and still require both
sets of signing secrets. They do not upload to TestFlight; the iOS job currently does not
save an IPA artifact.

Before using your own mobile signing identities in a fork, configure these together:

- Android: set `env.CERT_SHA256` in [`mobile.yml`](.github/workflows/mobile.yml) to your
  signing certificate's SHA-256 fingerprint. Configure the `ANDROID_KEYSTORE_BASE64`,
  `ANDROID_KEYSTORE_PASSWORD` and `ANDROID_KEY_ALIAS` repository secrets. The workflow
  uses `ANDROID_KEYSTORE_PASSWORD` for both the keystore and key passwords.
- iOS: set your application identifier and team in [`tauri.conf.json`](mobile/src-tauri/tauri.conf.json)
  and the same team in [`testflight.sh`](mobile/scripts/testflight.sh). Configure `ASC_KEY_ID`,
  `ASC_ISSUER_ID` and `ASC_KEY_P8` repository secrets; the signing script uses an API key
  with the permissions described in the [mobile release instructions](mobile/README.md#releasing).

The mobile version comes from `mobile/src-tauri/tauri.conf.json`; a `mobile-v<version>` tag
must match it. Desktop `v*` tags and mobile `mobile-v*` tags are versioned separately.

## Benchmarks

Same machine, same day, same 155×40 grid — Apple M1 Pro, macOS 26.3.1,
five-run averages (2026-07-04):

| | **tty7** | Alacritty | Ghostty | Kitty |
|---|---:|---:|---:|---:|
| Plaintext I/O — 11 MB `cat` <sub>(lower = better)</sub> | **95 ms** | 239 ms | 179 ms | 185 ms |
| [DOOM-fire](https://github.com/const-void/DOOM-fire-zig) frame rate <sub>(higher = better)</sub> | **888 fps** | 485 fps | 552 fps | 617 fps |
| Cold-launch memory | 116 MB¹ | 105 MB | 128 MB | 130 MB |

<sub>¹ GUI 105 MB + the persistent server 11 MB.</sub>

Methodology and one-command reproduction: [`scripts/bench/`](scripts/bench/README.md).

---

<div align="center">
<sub>

Built on [gpui](https://github.com/zed-industries/zed) and [`alacritty_terminal`](https://github.com/zed-industries/alacritty) · [Apache-2.0](LICENSE) · [Discord](https://discord.gg/s3dethqz2V) · [Changelog](CHANGELOG.md)

</sub>
</div>
