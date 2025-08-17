# SnapLock

**一个智能的安全工具，当您离开时，若有活动则自动拍照并锁屏。**

[![构建状态](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com)
[![许可证: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![最新版本](https://img.shields.io/github/v/release/OldSuns/snaplock)](https://github.com/OldSuns/snaplock/releases)

---

<img width="1090" height="1020" alt="image" src="https://github.com/user-attachments/assets/92cceb36-7db3-4a70-94f8-f72d32af086b" />

## 项目简介

SnapLock 是一款智能安全工具，当您离开电脑时，若检测到活动，它会自动拍照并锁定屏幕。该工具采用现代化技术栈构建，前端使用 Vue 3，后端采用 Rust 与 Tauri，为物理计算机安全提供了轻量高效的解决方案。

SnapLock 解决了一个常见的安全问题：当您需要短暂离开电脑，但又不想使用系统锁屏，同时又担心有人趁您不在时访问您的电脑，这时该怎么办？该应用程序通过在后台静默运行来解决这一问题，并能立即响应任何键盘或鼠标活动，采取两项安全措施：通过您选定的摄像头拍照并锁定计算机屏幕。

该应用程序设计为**非侵入式且资源高效**，利用 Rust 的性能和 Tauri 的轻量架构，确保对系统资源的影响最小化，同时提供强大的安全监控功能。

## 主要功能
SnapLock 提供多项核心功能，使其既强大又用户友好：

1. 智能监控：经过 2 秒的准备期后，SnapLock 进入主动监控模式，随时准备检测任何输入活动。
2. 活动触发响应：任何键盘或鼠标事件都会立即触发安全响应，确保未经授权的访问不会被忽视。
3. 即时拍照：在锁定屏幕前，SnapLock 通过您选定的摄像头拍摄照片，为事件创建视觉记录。
4. 全局热键：在系统任何位置使用 Alt+L（可自定义）快速启动或停止监控。
5. 系统托盘操作：应用程序在后台运行，并显示系统托盘图标，允许隐藏主界面而不影响您的工作流程。
6. 高度可配置：可选择多个摄像头，并为拍摄的照片设置自定义保存路径。
7 .轻量高效：基于 Rust 和 Tauri 构建，确保资源使用最少，同时保持高性能。

## SnapLock 如何工作
SnapLock 通过简化的工作流程运行，旨在以最少的用户努力实现最大安全性：

1. **配置阶段：**
   - 用户在主界面的下拉列表中选择首选摄像头。
   - 可选择设置自定义文件夹保存拍摄的照片（默认为桌面）。

2. **准备阶段：**
   - 用户按下全局热键 Alt+L（可自定义）或点击"开始监控"按钮。
   - 应用程序状态变为"准备中"，提供 2 秒缓冲时间以防止意外触发。

3. **主动监控阶段：**
   - 准备期结束后，SnapLock 进入"活跃"状态，主窗口自动隐藏。
   - 应用程序静默监控所有系统范围的键盘和鼠标输入事件。

4. **触发与行动阶段：**
   - 检测到任何键盘或鼠标活动时：
   - 拍照：立即通过选定的摄像头拍摄照片并保存到指定路径。
   - 屏幕锁定：执行系统命令锁定计算机屏幕。

## 技术架构
SnapLock 采用现代化混合架构构建，结合了 Web 技术的用户界面优势和系统编程的性能优势：

### 前端（Vue 3 + TypeScript）
用户界面使用 Vue 3 和 TypeScript 构建，提供响应式且直观的体验。前端负责：

- 用户交互和配置管理
- 显示应用程序状态和摄像头预览
- 处理设置和偏好
- 为所有操作提供视觉反馈
前端通过 Tauri 的调用系统与后端通信，实现 Web 界面与原生功能的无缝集成。

### 后端（Rust + Tauri）
后端使用 Rust 构建，并采用 Tauri 框架创建轻量、安全且高性能的原生应用程序。Rust 后端处理：

- 使用 rdev crate 进行系统级输入监控
- 通过 nokhwa crate 控制摄像头和拍照
- 系统屏幕锁定操作
- 配置持久化管理
- 全局热键注册和处理
- 系统托盘集成
- 日志记录和诊断

## SnapLock 入门指南
SnapLock 设计为即装即用，设置简单：

- **安装**：从 GitHub Releases 页面下载最新安装程序。
- **配置**：启动 SnapLock 并从下拉列表中选择首选摄像头。可选择自定义拍摄照片的保存路径。
- **使用**：当您需要离开电脑时，只需按下全局热键（默认：Alt+L）。应用程序将提供 2 秒准备时间，然后进入主动监控模式。如果有人在您离开时尝试使用您的电脑，SnapLock 会拍摄他们的照片并立即锁定屏幕。

## 安全考虑
SnapLock 将安全性作为首要考虑因素：

- **本地运行**：所有处理都在您的本地机器上完成 - 不会向外部服务器发送任何数据。
- **最小权限**：应用程序仅请求摄像头访问和系统监控的必要权限。
- **透明操作**：用户可以通过状态指示器和系统托盘图标完全了解监控何时处于活动状态。
- **可配置存储**：用户控制拍摄照片的存储位置，确保隐私和数据所有权。
SnapLock 代表了一种深思熟虑的物理计算机安全方法，在强大保护与用户便利性和系统效率之间取得了平衡。

## 开发与贡献

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
