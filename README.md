# SnapLock

**一个智能的安全工具，当您离开时，若有活动则自动拍照并锁屏。**

[![构建状态](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com)
[![许可证: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![最新版本](https://img.shields.io/github/v/release/OldSuns/snaplock)](https://github.com/OldSuns/snaplock/releases)

---

<img width="493" height="497" alt="image" src="https://github.com/user-attachments/assets/305f5a4d-f161-4044-9b55-eec7ada12c88" />

## 📖 项目简介

您是否曾短暂离开座位但**不方便使用系统锁屏**，却担心有人在您离开时偷看或操作您的电脑？

**SnapLock** 解决了这个问题。它是一个轻量级的桌面应用，当您准备离开时，可以一键启动“警戒”模式。此后，它会在后台静默运行。一旦检测到任何键盘或鼠标活动（表明可能有人正在使用您的电脑），SnapLock 会立即通过摄像头拍摄一张照片，然后锁定您的计算机，最后自动退出。

这个过程确保了在您不知情的情况下，任何对您电脑的物理访问都会被记录下来，并立即被阻止。

## ✨ 核心功能

*   **智能监控**: 在启动的两秒钟准备后进入警戒模式。
*   **活动触发**: 任何键盘或鼠标事件都会立即触发安全响应。
*   **即时拍照**: 在锁屏前通过选定的摄像头捕捉一张照片，作为事件记录。
*   **全局快捷键**: 使用 `Alt+L` 在任何地方都能快速启动或停止监控。（程序内可修改快捷键）
*   **系统托盘运行**: 应用在后台运行，主界面可以随时隐藏，不干扰您的工作区。
*   **高度可配置**: 支持在多个摄像头之间进行选择，并可以自定义照片的保存路径。
*   **轻量高效**: 基于 Rust 和 Tauri 构建，资源占用极低。

## ⚙️ 工作流程

SnapLock 的设计哲学是“一次性任务，执行后即销毁”。它的工作流程如下：

1.  **配置阶段**:
    *   用户在主界面中，从下拉列表选择要使用的摄像头。
    *   （可选）设置一个自定义的文件夹用于保存捕获的照片，默认为桌面。

2.  **布防阶段 (Arming)**:
    *   用户按下全局快捷键 `Alt+L` （可修改）或点击界面上的“启动监控”按钮。
    *   应用状态变为“准备中”，并给予用户几秒钟的准备时间离开座位。

3.  **警戒阶段 (Active)**:
    *   准备时间结束后，应用进入“警戒中”状态，主窗口自动隐藏。
    *   此时，SnapLock 在后台静默监听系统范围内的所有键盘和鼠标输入事件。

4.  **触发与执行阶段 (Trigger & Action)**:
    *   一旦检测到**任何**键盘或鼠标活动：
        *   **拍照**: 立即通过选定的摄像头拍摄一张照片，并保存到指定路径。
        *   **锁屏**: 执行系统命令锁定计算机屏幕。
        *   **退出**: 完成任务后，应用进程会自动终止，不留任何后台服务。

## 🚀 使用教程

### 1. 安装

您可以从我们的 [GitHub Releases](https://github.com/OldSuns/snaplock/releases) 页面下载最新的安装程序（例如 `SnapLock_0.4.0_x64-setup.exe`）。

### 2. 配置

1.  启动 SnapLock。
2.  点击**启动监控**或使用自定义快捷键（默认为`Alt+L`）。
3.  （可选）在 **“启动监控”** 旁边，点击 `设置` 按钮，打开自定义设置。

<img width="496" height="495" alt="image" src="https://github.com/user-attachments/assets/2ff75ec8-1082-471d-8028-d1407bdef913" />

### 3. 使用

1.  当您准备临时离开电脑时，按下自定义快捷键。
2.  应用状态会变为“准备中”，您有几秒钟的时间离开。
3.  当状态变为“警戒中”后，您可以放心离开。
4.  如果有人在您离开时试图使用您的电脑，SnapLock 会立即拍照并锁屏。
5.  您回来后，正常解锁电脑即可。捕获的照片可以在您之前设定的路径中找到。

## 🛠️ 技术栈

*   **后端**: [Rust](https://www.rust-lang.org/)
    *   **框架**: [Tauri](https://tauri.app/)
    *   **异步运行时**: [Tokio](https://tokio.rs/)
    *   **摄像头控制**: [`nokhwa`](https://crates.io/crates/nokhwa)
    *   **全局输入监听**: [`rdev`](https://crates.io/crates/rdev)
*   **前端**:
    *   **框架**: [Vue 3](https://vuejs.org/) with [TypeScript](https://www.typescriptlang.org/)
    *   **构建工具**: [Vite](https://vitejs.dev/)
    *   **CSS 框架**: [Pico.css](https://picocss.com/)

## 💻 开发与贡献

我们欢迎任何形式的贡献！

### 环境准备

*   [Node.js](https://nodejs.org/) 和 [pnpm](https://pnpm.io/)
*   [Rust](https://www.rust-lang.org/tools/install) 环境

### 运行开发环境

```bash
# 1. 克隆仓库
git clone https://github.com/OldSuns/snaplock.git
cd snaplock

# 2. 安装前端依赖
pnpm install

# 3. 启动开发服务器
pnpm tauri dev
```

如果您有任何建议或发现 Bug，请随时提交 [Issues](https://github.com/OldSuns/snaplock/issues) 或 [Pull Requests](https://github.com/OldSuns/snaplock/pulls)。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=OldSuns/SnapLock&type=Date)](https://www.star-history.com/#OldSuns/SnapLock&Date)

## 📄 许可证

本项目基于 [MIT License](LICENSE) 开源。
