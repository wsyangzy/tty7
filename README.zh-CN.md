<div align="center">

<img src="assets/app-icon.svg" alt="tty7" width="88" height="88" />

### tty7

**终端工作台：会话常驻、远程开发、原生支持 agent。**

<sub>纯 Rust · GPU 渲染基于 Zed 的 gpui · VT 内核来自 Alacritty</sub>

<br />

[![CI](https://github.com/l0ng-ai/tty7/actions/workflows/ci.yml/badge.svg)](https://github.com/l0ng-ai/tty7/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/l0ng-ai/tty7?label=version&color=3FDD8C)](https://github.com/l0ng-ai/tty7/releases)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-blue)](https://github.com/l0ng-ai/tty7/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-%E5%8A%A0%E5%85%A5%E8%AE%A8%E8%AE%BA-5865F2?logo=discord&logoColor=white)](https://discord.gg/s3dethqz2V)

<sub>[English](README.md) · 简体中文</sub>

<br />

<a href="assets/tour.mp4"><img src="assets/tour.webp" alt="tty7 一分钟演示：多仓库 agent、一个 agent 通过 CLI 驱动另一个、提示符编辑器、diff、拖动 pane、退出应用后会话仍在运行" width="900" /></a>

<sub>▶ <a href="assets/tour.mp4">观看高清版</a></sub>

</div>

## 为什么

shell 和窗格由后台常驻的 server 持有，而不是窗口。下面这些特性几乎都源自这一设计。

- **性能**：吞吐是 Alacritty、Ghostty、Kitty 的两倍左右（[基准测试](#基准测试)）
- **会话常驻**：退出应用，shell 照常运行；重启机器后，布局、屏幕内容和已支持的 agent 对话都会回来。不需要 tmux
- **Agent 感知**：Claude Code、Codex 等 agent 的状态、通知和 git 上下文，多个仓库一屏看完
- **可被 agent 驱动**：一个 agent 能给另一个开 pane、派活、等它跑完、读走结果，GUI 开不开都行
- **编辑器级输入**：建议、补全、高亮、历史搜索，不用装任何插件
- **远程开发**：文件、仓库、pane 和 git 信息都留在远端机器上，走自带的 SSH 栈

## 安装

macOS、Windows、Linux 的原生构建都在 [**Releases**](https://github.com/l0ng-ai/tty7/releases)：

| | | |
|---|---|---|
| **macOS** | `…-macos-arm64.dmg` · `…-x86_64.dmg` | 拖进「应用程序」 |
| **Windows** | `…-setup.exe` · 免安装 `….zip` | |
| **Linux** | `…-x86_64.AppImage` | `chmod +x` 后直接运行，X11/Wayland 的库已打包在内 |

桌面端以及 Android、iOS 手机端的本地构建命令见[从源码编译](#从源码编译)。

## 有什么

| | |
|---|---|
| **Agent 感知** | 逐 pane 识别 28 个 CLI agent · 状态点 · 通知 · 分支 + diff · 需要输入时托盘图标提醒 · 重启后续上会话 · 在随处搜索里找回任意历史会话 · 一键启动 · 侧边栏按仓库分组，可置顶 |
| **CLI + Skills** | 安装包自带 `tty7` CLI · [agent skill](skills/tty7/SKILL.md) · `run` 转发命令输出并原样返回退出码 · `split` · `send` · `wait --until free` · `capture` |
| **编辑器级输入** | 基于历史的内联补全建议 · Tab 补全附带说明 · 语法高亮 · 多行编辑 · 点击定位光标 · <kbd>⌃ R</kbd> 模糊搜索历史 |
| **窗口** | 标签页与分屏 · <kbd>⌘ P</kbd> 随处搜索 · <kbd>⌘ F</kbd> 回滚搜索 · 休眠标签页以释放内存 · 输入法 |
| **侧边面板** | <kbd>⌘ J</kbd> · 进程树与监听端口 · 文件树 · 改动与 diff · 在文件中查找 · GitHub issue 与 PR（只读，通过 `gh` 登录） |
| **Shell 集成** | pane 启动时自动注入，不用你装什么 · 提示符边界 · 工作目录 · 退出码 · 命令跑完发通知 · 覆盖 zsh、bash、fish、PowerShell、WSL 和远程 pane |
| **远程工作区** | 远端的文件、仓库、改动、diff、worktree、标签页和 pane · 从任意客户端重连，接着离开时的位置继续 |
| **SSH** | 自带 russh 实现，不依赖外部 ssh：profile 凭据存入 keychain · SFTP 面板 · 端口转发 · 跳板机 · `tty7-server` 只需安装一次，无需 root |

## 支持的 agent

**识别**无需配置：品牌头像、分支与 diff、标签页标题。
**状态**需要在设置 → 集成 中为该 agent 安装 hook，一次点击，之后才有状态点、通知、托盘提醒、`tty7 wait` 和重启后恢复会话。
**Fork** 两个条件都要：agent 自己提供 fork 命令，且 hook 已装——tty7 得知道 fork 的是哪个会话。
**历史会话**从 agent 自己的历史文件读取，列在随处搜索里，<kbd>⏎</kbd> 即可续上。

<details>
<summary>28 个 agent 的完整支持矩阵</summary>

| Agent | 识别 | 状态 · 重启恢复 | Fork | 历史会话 |
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

tty7 不包装、不代理其中任何一个 —— 你启动的就是那个 agent 本身，运行在普通 PTY 中，界面仍然是它自己的。
如果你通过 wrapper 脚本启动 agent，在 `config.json` 的 `agent_commands` 里把脚本名映射到对应 agent 即可。
PATH 上的每个 agent 也都能在随处搜索里找到（**Agent：Claude Code** 等），会在新标签页中启动它；新建标签页菜单会列出真正用过的那几个。`agent_launch` 可以设置它的启动命令行。

## 文档

完整文档在 [**`docs/`**](docs/)，英文：
[快捷键](docs/reference/keyboard-shortcuts.mdx) ·
[config.json](docs/reference/configuration.mdx) ·
[CLI 参考](docs/cli/reference.mdx)。
agent 如何调用这套 CLI，另见 [skills/tty7/SKILL.md](skills/tty7/SKILL.md)。

安装 skill：

```sh
npx skills add l0ng-ai/tty7    # 安装
npx skills update tty7         # 后续更新
```

## 从源码编译

先安装 Git 和通过 `rustup` 管理的当前稳定版 Rust。每个命令块都从项目根目录开始，
在对应操作系统和指定的 shell 中执行；手机端命令随后进入 `mobile/` 安装前端依赖。
`rustup target add` 只添加 Rust 目标库，仍需安装对应平台的编译工具链和 SDK。

桌面端命令会编译 `tty7-app`、`tty7` CLI、`tty7-server`，随后单独编译 `tty7-updater`。
产物位于 `target/<target>/release/`，Windows 下带 `.exe` 后缀。
这些命令生成可执行文件；安装包由 [GitHub Actions 工作流](#github-actions)的打包步骤生成。

### Windows

在 PowerShell 中执行。需要 Visual Studio Build Tools 的 **使用 C++ 的桌面开发**组件、
Windows SDK 和 CMake。ARM64 还需要 MSVC ARM64 编译组件，目前属于实验性桌面发布目标。

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

在安装了 Xcode 及其命令行工具的 macOS 上，使用 Bash 或 Zsh 执行。
按运行应用的 Mac 架构选择对应目标。

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

### Linux 桌面端

桌面发布目标为 Linux x64。Ubuntu / Debian 可通过以下命令安装编译依赖；
其他发行版需要安装对应的软件包。

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

### 独立服务端

桌面构建已经包含对应平台的 `tty7-server`。只需要服务端时，可以单独选择该包。
Windows x64 不选 GUI 包单独构建时，还需要将 NASM 加入 `PATH`。
先满足上面的 Windows 编译前提，再在 PowerShell 中执行：

```powershell
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
cargo build --release --locked -p tty7-server --target x86_64-pc-windows-msvc
cargo build --release --locked -p tty7-server --target aarch64-pc-windows-msvc
```

macOS 上满足前述 Xcode 环境要求后执行：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked -p tty7-server --target aarch64-apple-darwin
cargo build --release --locked -p tty7-server --target x86_64-apple-darwin
```

Linux x64 / ARM64 静态服务端使用 Linux 构建环境，需要 C/C++ 工具链、CMake，
以及已加入 `PATH` 的 Zig **0.16.0** 和 `cargo-zigbuild`，与工作流保持一致。
保留 `-p tty7-server`，因为 GUI 启用的 GSSAPI 依赖无法用于 musl 构建。

```bash
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo zigbuild --release --locked -p tty7-server --target x86_64-unknown-linux-musl
cargo zigbuild --release --locked -p tty7-server --target aarch64-unknown-linux-musl
```

产物位于 `target/<target>/release/`。Linux ARM64 属于服务端目标；
当前桌面发布矩阵中的 Linux 目标只有 x64。

### 独立手机网关

GUI 已内嵌手机网关，在 **设置 → 手机 → 允许手机访问** 中开启。
单独运行 `tty7-server` 时，需要将 `tty7-gateway` 放在服务端旁边或加入 `PATH`，才能接受手机连接。
默认桌面构建不会另外生成独立的网关可执行文件。

Windows 下同时选择 GUI 和网关包，复用 GUI 启用的依赖特性，其中 x64 使用
`aws-lc-sys/prebuilt-nasm` 提供的预编译汇编对象。环境要求与上面的 Windows 桌面构建相同：

```powershell
# Windows x64
rustup target add x86_64-pc-windows-msvc
cargo build --release --locked -p tty7 -p tty7-gateway --target x86_64-pc-windows-msvc

# Windows ARM64；需要安装对应 Rust target 和 MSVC ARM64 编译组件
rustup target add aarch64-pc-windows-msvc
cargo build --release --locked -p tty7 -p tty7-gateway --target aarch64-pc-windows-msvc
```

macOS / Linux 上准备好对应平台的本地编译工具后，编译当前宿主架构：

```bash
cargo build --release --locked -p tty7-gateway
```

Windows 产物为 `target/<target>/release/tty7-gateway.exe`；
上面的 macOS / Linux 本机构建产物为 `target/release/tty7-gateway`。

### Android

手机客户端位于 [`mobile/`](mobile/README.md)。需要当前 Node.js 22 版本、JDK **17–21**、
Android SDK 和 NDK，并设置 `JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME`。
手机工作流固定使用 NDK **28.2.13676358**。以下以 Windows PowerShell 为例，
配置好相应环境的 macOS / Linux 也可使用这些命令。

Android 原生工程已提交到仓库，其中包含键盘处理定制，**不要重新执行 `android init`**。
从项目根目录执行：

```powershell
cd mobile
rustup target add aarch64-linux-android
npm ci

# ARM64 Debug APK：使用本机调试密钥签名，可安装到手机
npx tauri android build --debug --apk --target aarch64

# ARM64 Release APK：需要先配置 keystore.properties 才会生成签名 APK
npx tauri android build --apk --target aarch64
```

APK 位于 `mobile/src-tauri/gen/android/app/build/outputs/apk/` 下。
正式签名需要创建已被 Git 忽略的 `mobile/src-tauri/gen/android/keystore.properties`，
并填写自己的值：

```properties
storeFile=C:/keys/tty7-release.jks
storePassword=<store-password>
keyAlias=<key-alias>
keyPassword=<key-password>
```

`storeFile` 使用本机密钥库的绝对路径。未配置该文件时，Release APK 不带签名；
Debug APK 使用调试签名，无需配置发布密钥即可安装。

### iOS

必须在 **macOS 和完整 Xcode** 环境下构建，并具备对应签名团队的 Apple 开发者账号权限。
工作流使用 Xcode 26。fork 自用时，将 [`tauri.conf.json`](mobile/src-tauri/tauri.conf.json)
中的 `identifier`、`bundle.iOS.developmentTeam` 改为自己的应用标识和团队，
并同步修改 [`scripts/testflight.sh`](mobile/scripts/testflight.sh) 中的 `TEAM`。

在 Xcode 中登录对应团队账号，或通过 `ASC_KEY_ID`、`ASC_ISSUER_ID`、`ASC_KEY_PATH`
提供 App Store Connect API 密钥。从项目根目录执行：

```bash
cd mobile
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
npm ci

# 编译、归档并导出 App Store 签名 IPA，不上传 TestFlight
bash scripts/testflight.sh --no-upload
```

脚本会在 iOS 工程缺失时自动初始化，IPA 输出到 `mobile/build/testflight/`。
省略 `--no-upload` 会上传至 TestFlight。
设备和模拟器开发命令见[手机端 README](mobile/README.md)。

### GitHub Actions

仓库工作流已覆盖上述桌面、服务端和手机端目标。GitHub 使用已推送引用中的工作流文件；
本地修改和提交需要推送后才会出现在 GitHub 上。

| 工作流 | 触发方式 | 产物与行为 |
|---|---|---|
| [`ci.yml`](.github/workflows/ci.yml) | 推送到 `main`、PR 或手动运行 | 编译与检查任务 |
| [`release.yml`](.github/workflows/release.yml) | `v*` 标签或手动运行 | 桌面安装包和独立服务端 artifacts；标签运行还会组装 Release 草稿 |
| [`nightly.yml`](.github/workflows/nightly.yml) | 每日 UTC 18:00 / UTC+8 02:00，或手动运行 | 有新提交时构建并更新滚动的 `nightly` 预发布版本 |
| [`mobile.yml`](.github/workflows/mobile.yml) | `mobile-v*` 标签或手动运行 | 签名 Android ARM64 APK artifact 和 iOS 构建；标签运行还会将 APK 附加到 Release 草稿并将 iOS 上传至 TestFlight |

桌面自用打包可在已推送的分支上手动运行 **Release**，再下载对应 artifact，
例如 `release-windows-x86_64`。普通分支的手动运行不会创建稳定版 Release。
Mobile 手动运行仍会构建 Android 和 iOS 两端，并要求两端签名 Secrets 齐全；
此时不会上传 TestFlight，当前 iOS job 也没有保存 IPA artifact 的步骤。

fork 使用自己的手机端签名身份时，需要同时配置：

- Android：将 [`mobile.yml`](.github/workflows/mobile.yml) 中的 `env.CERT_SHA256`
  改为自己的签名证书 SHA-256 指纹，并配置仓库 Secrets：`ANDROID_KEYSTORE_BASE64`、
  `ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`。工作流将 `ANDROID_KEYSTORE_PASSWORD`
  同时用作密钥库密码和密钥密码。
- iOS：在 [`tauri.conf.json`](mobile/src-tauri/tauri.conf.json) 中配置自己的应用标识和团队，
  并同步 [`testflight.sh`](mobile/scripts/testflight.sh) 的团队设置。配置仓库 Secrets：
  `ASC_KEY_ID`、`ASC_ISSUER_ID`、`ASC_KEY_P8`；API 密钥权限要求见
  [手机端发布说明](mobile/README.md#releasing)。

手机版本号以 `mobile/src-tauri/tauri.conf.json` 为准，`mobile-v<version>` 标签必须与其一致。
桌面 `v*` 标签与手机 `mobile-v*` 标签分别管理版本。

## 基准测试

同一台机器、同一天、同样的 155×40 网格：Apple M1 Pro，macOS 26.3.1，每项运行五次取平均（2026-07-04）。

| | **tty7** | Alacritty | Ghostty | Kitty |
|---|---:|---:|---:|---:|
| 纯文本 I/O：`cat` 一个 11 MB 文件 <sub>（越低越好）</sub> | **95 ms** | 239 ms | 179 ms | 185 ms |
| [DOOM-fire](https://github.com/const-void/DOOM-fire-zig) 帧率 <sub>（越高越好）</sub> | **888 fps** | 485 fps | 552 fps | 617 fps |
| 冷启动内存 | 116 MB¹ | 105 MB | 128 MB | 130 MB |

<sub>¹ GUI 占 105 MB，常驻 server 占 11 MB。</sub>

测试方法与一条命令复现：[`scripts/bench/`](scripts/bench/README.md)。

---

<div align="center">
<sub>

基于 [gpui](https://github.com/zed-industries/zed) 和 [`alacritty_terminal`](https://github.com/zed-industries/alacritty) 构建 · [Apache-2.0](LICENSE) · [Discord](https://discord.gg/s3dethqz2V) · [更新日志](CHANGELOG.md)

</sub>
</div>
