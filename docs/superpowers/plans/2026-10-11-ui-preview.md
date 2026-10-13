# WinSpeaker UI Preview Implementation Plan

**Goal:** 交付无需音频接口的 Rust 原生可交互预览，固定路径 WinSpeaker.exe。

**Architecture:** 原生 Win32 消息循环，GDI 双缓冲绘制，模型与绘制分离；无第三方运行时、无持续刷新。第一阶段仅 mock，确认 UI 后才接入真实音频。

**Tech Stack:** Rust stable、Win32/GDI/Shell 通知区域；使用系统链接库，不下载 Rust 依赖。

**Spec:** [需求开发文档](../../../需求开发文档.md)。用户已回复“开发”，在本会话直接执行，不再次请求开发授权。

## Global Constraints

- 不运行测试；构建固定 EXE 供用户验证。
- 仅 main；当前没有 Git 仓库或远程地址，不虚构同步完成状态。
- UI 确认前不接入真实音量、应用路由、开机启动设置。
- 原图为视觉依据；按 DPI 缩放；图标来自 pic/icon.png。

## Review Focus

用户验证：高 DPI/小工作区滚动；长设备名截断；滑块边界和鼠标捕获释放；托盘隐藏恢复与退出；五类状态和设置中的预览提示。

## Task 1：模型与原生界面

Files: Cargo.toml, src/main.rs, src/win32.rs, src/model.rs, src/ui.rs, src/paint.rs。

- [x] 定义 Device、App、Scenario 与 mock 数据，包括 normal/loading/empty/error/edge。
- [x] 建立事件驱动窗口与 DPI 缩放、双缓冲绘制、设备折叠、滚动与滑块操作。
- [x] 使用原图图标和已安装应用的图标，仅做 UI 资源读取。

## Task 2：菜单与预览设置

- [x] 托盘左键显示面板，右键显示深色设备菜单；关闭面板仍常驻，退出释放资源。
- [x] 默认设备、应用设备菜单和开机启动在内存中模拟，明确标注预览。
- [x] 状态切换可通过底部入口与 F1 触发；错误提供恢复按钮。

## Task 3：构建与交付

Files: build.bat, build.rs, resources/app.manifest, tools/make_resources.py, tools/build.py, README.md, 开发日志.md。

- [x] 从原 PNG 封装 Windows ICO，嵌入 EXE 图标与 DPI/版本资源。
- [x] 构建 release EXE，停止已运行的同名应用，覆盖项目根目录 WinSpeaker.exe。
- [x] 根据编译输出和文件属性确认交付文件，记录阶段完成与尚未实现功能。
- [x] 交付用户验证；用户确认 UI 后继续音频模块。

## 执行记录

- 直接在当前工作区执行；无功能分支或 worktree。当前无 Git 仓库及远程地址，因此没有提交或推送。
- 按用户明确要求不运行测试、不启动 EXE；只进行 release 编译、源码审查、文件版本与导入 DLL 元数据读取。
- 原 MSVC 缺少链接器及 SDK；经批准安装官方 GNU Rust 工具链，使用其自带 self-contained GCC。
- GNU 工具链无 windres：资源生成器直接输出 Windows COFF .rsrc 对象，避免另行安装 SDK 或资源编译器。
- 源码审查后已修正：设置页滚动、菜单尺寸限制与滚动、弹出菜单独立 DPI、默认设备切换清理旧焦点、注册消息缓存。
- 后续门槛来自 dev规范.txt：用户确认 UI 效果后才能接入真实音频业务。
