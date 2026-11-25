# SnapLock

Windows 活动监控工具：检测键盘或鼠标输入后，按设置拍摄、锁屏或录制屏幕。基于 Rust、Tauri 2、Vue 3 和 TypeScript，照片、录像与日志保存在本地。

[![许可证: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![最新版本](https://img.shields.io/github/v/release/OldSuns/snaplock)](https://github.com/OldSuns/snaplock/releases)

<img width="1090" height="1020" alt="SnapLock 界面" src="https://github.com/user-attachments/assets/92cceb36-7db3-4a70-94f8-f72d32af086b" />

## 使用

1. 从 [Releases](https://github.com/OldSuns/snaplock/releases) 下载 Windows 安装包。
2. 打开设置，选择触发后动作。使用摄像头拍摄时，选择摄像头并通过预览确认权限与画面。
3. 点击“启动监控”或按默认快捷键 `Alt+L`。准备 2 秒后进入警戒状态，主窗口自动隐藏。
4. 使用同一快捷键或托盘菜单停止监控；点击托盘图标可重新显示窗口。关闭主窗口会隐藏到托盘，退出程序请使用托盘菜单。

| 触发后动作 | 行为 |
| --- | --- |
| 拍摄并锁屏 | 摄像头拍摄完成后锁定 Windows |
| 仅拍摄 | 保存照片或摄像头录像，随后停止监控 |
| 屏幕录制 | 输入活动触发录屏；无输入超过 20 秒时结束当前录像，再次活动时继续录制，无需摄像头 |

Windows 解锁后，应用会停止监控并清理录制进程。

## 设置与文件

- **开机自启动**：在设置的“启动选项”中开启或关闭，默认关闭。开启后，登录 Windows 时自动打开 SnapLock，随后可通过界面、快捷键或托盘启动监控。
- **拍摄时长**：设置为 0 秒时拍照，1–60 秒时录制对应时长的摄像头视频；此设置不影响屏幕录制。
- **保存目录**：默认为桌面，可在设置中更改。启用文件日志后，日志保存在同一目录；照片与录像使用唯一文件名并禁止覆盖。
- **快捷键**：默认 `Alt+L`，可自定义。启动时若快捷键被占用，会尝试备用组合。
- **其他选项**：默认摄像头、明暗主题、系统通知、触发后退出，以及实时日志显示和文件保存。

开机自启动由系统保存，打开设置时读取当前状态，修改失败时显示错误。其他设置即时保存到 `%APPDATA%\SnapLock\snaplock_config.json`，兼容旧配置。保存失败时保留原值并显示错误。

屏幕录像初始化失败或录制进程意外退出时，应用会停止监控并显示错误。停止录像会先请求 FFmpeg 完成文件写入，超时后强制终止；异常终止的录像可能不完整。

## 开发

需要 Windows、[Node.js](https://nodejs.org/)、[pnpm](https://pnpm.io/)、[Rust](https://www.rust-lang.org/tools/install) MSVC 工具链、C++ Build Tools 和 WebView2，安装步骤见 [Tauri 环境准备](https://v2.tauri.app/start/prerequisites/)。项目已包含 FFmpeg。

```powershell
git clone https://github.com/OldSuns/snaplock.git
cd snaplock
pnpm.cmd install --frozen-lockfile
pnpm.cmd tauri dev
```

### 检查与测试

```powershell
pnpm.cmd build
cargo test --locked --manifest-path src-tauri/Cargo.toml --bin SnapLock
```

`pnpm.cmd build` 执行前端类型检查和构建。Rust 测试覆盖配置校验与持久化、监控状态与任务取消、会话消息过滤，以及 FFmpeg 启动、停止、异常退出、防覆盖和完整解码。

测试前需创建 `D:\Temp\Pi`，测试产物保存在其下的 `snaplock-*` 路径。自动测试不覆盖真实系统锁屏与解锁，摄像头预览、拍摄和实际输入触发需另行实机验证。

### 构建安装包

```powershell
pnpm.cmd tauri build
```

该命令会自动构建前端，Windows 安装包输出到 `src-tauri/target/release/bundle/nsis/`。目录运行时需保留可执行文件旁的 `libs/ffmpeg` 资源。

### 代码结构

| 位置 | 职责 |
| --- | --- |
| [src/components/](src/components/) 与 [src/composables/](src/composables/) | 设置、摄像头预览、日志界面与前端状态；`App.vue` 组合主窗口 |
| [config.rs](src-tauri/src/config.rs) 与 [state.rs](src-tauri/src/state.rs) | 配置校验、保存与运行状态 |
| [monitoring/](src-tauri/src/monitoring/) | 监控生命周期、输入监听与触发动作，统一处理取消和清理 |
| [camera.rs](src-tauri/src/camera.rs)、[recorder.rs](src-tauri/src/recorder.rs) 与 [process_utils.rs](src-tauri/src/process_utils.rs) | 摄像头操作、录像进程管理、启动确认与停止清理 |
| [session_monitor.rs](src-tauri/src/session_monitor.rs) | 当前 Windows 会话的锁定与解锁通知 |

问题反馈请提交 [Issues](https://github.com/OldSuns/snaplock/issues)，代码贡献请提交 [Pull Requests](https://github.com/OldSuns/snaplock/pulls)。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=OldSuns/SnapLock&type=Date)](https://www.star-history.com/#OldSuns/SnapLock&Date)

## 许可证

本项目采用 [MIT License](LICENSE)。
