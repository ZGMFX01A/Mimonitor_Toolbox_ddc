# 第三方开源许可证与版权声明 (Third-Party Notices)

> 本文件随 **Mimonitor Toolbox** (Rust + GPUI Windows 客户端) 及其便携分发包一同提供。  
> 本项目完全基于原生 Rust 与 Win32 API 构建，运行时**不包含** Python、Qt、Node.js、Electron 或 Java 虚拟机运行时。  
> 除随包分发的标准开源 Android SDK Platform-Tools ADB 客户端之外，所有业务逻辑均由原生二进制执行。

---

## 目录
1. [核心参考与直接分发组件](#1-核心参考与直接分发组件)
2. [伴随运行辅助工具说明](#2-伴随运行辅助工具说明)
3. [Rust 运行时与编译依赖清单](#3-rust-运行时与编译依赖清单)
4. [未随包附带独立许可文本的依赖明示](#4-未随包附带独立许可文本的依赖明示)
5. [通用开源许可证全文条款](#5-通用开源许可证全文条款)

---

## 1. 核心参考与直接分发组件

### 1.1 Twinkle Tray (DDC/CI 硬件通信参考)
- **许可证**: MIT License
- **版权所有**: Copyright (c) 2019 Xander Frangos
- **许可证全文文件**: `licenses/components/LICENSE-twinkle-tray.txt`
- **使用范围与说明**: 本项目仅在 `src/ddc/` 模块中参考并移植了 Twinkle Tray 的 Win32 DDC/CI 监视器枚举、VCP 0x10 回退读取与重试机制。本项目**没有**引入 Twinkle Tray 的 Electron、Node.js 运行时或前端界面代码。

### 1.2 node-ddcci (@hensm/ddcci)
- **许可证**: MIT License
- **版权所有**: Portions Copyright (c) Matt Hensman
- **许可证全文文件**: `licenses/components/LICENSE-node-ddcci.txt`
- **使用范围与说明**: 本项目参考了该项目整理的 Windows DDC/CI 瞬态错误代码（`0xC0262582` 至 `0xC0262587`）及重试判定，完全使用 Rust 原生实现，不包含任何 Node.js 原生扩展。

### 1.3 GPUI & Zed (UI 框架与组件参考)
- **许可证**: Apache License, Version 2.0
- **版权所有**: Copyright (c) Zed Industries, Inc.
- **许可证全文文件**: `licenses/components/LICENSE-gpui-input.txt`
- **使用范围与说明**: 本项目使用 GPUI 0.2.2 作为基于 Windows Direct3D 的原生 GPU UI 框架。`src/ui/components/text_input.rs` 借鉴并适配了 GPUI 官方 `input.rs` 示例的 IME 选区、焦点与文本交互逻辑，严格遵守 Apache-2.0 署名与保留条款。

### 1.4 Android SDK Platform-Tools (ADB 客户端及动态库)
- **许可证**: Apache License, Version 2.0
- **版权所有**: Copyright (C) The Android Open Source Project (Google LLC)
- **包含文件**: `assets/runtime/adb.exe`, `assets/runtime/AdbWinApi.dll`, `assets/runtime/AdbWinUsbApi.dll`
- **许可证全文文件**: `licenses/components/LICENSE-android-platform-tools.txt`
- **使用范围与说明**: 随程序分发未修改的原生 Windows x64 ADB 客户端，用于通过独立隔离端口（TCP 5038）与小米/红米显示器建立通信。

### 1.5 Microsoft Visual C++ Runtime (VCRUNTIME140.dll)
- **许可属性**: 微软专有软件可再分发代码 (Microsoft Visual Studio Redistributable Code)
- **版权所有**: Copyright (C) Microsoft Corporation. All rights reserved.
- **包含文件**: `vcruntime140.dll`（App-Local 存放于便携包根目录与 `assets/runtime/` 目录）
- **许可说明文件**: `licenses/components/NOTICE-msvc-redist.txt` 与 `licenses/components/Redist.txt`
- **分发说明**: 依据 Microsoft Visual Studio 软件许可条款，本项目采用 App-Local 方式随包分发匹配 x64 架构的官方可再分发动态库 `vcruntime140.dll`，使程序在未预装 Visual C++ Redistributable 的干净 Windows 10/11 操作系统上能够自足开箱即用。本组件属于微软专有可再分发组件，受微软许可条款约束，**绝非** MIT/Apache 等开源软件。

---

## 2. 伴随运行辅助工具说明

本项目包含以下针对显示器端（Android runtime）运行的辅助工具，继承自原项目并保留其源码归属：

- **MtkDirectTool.jar** (`assets/runtime/MtkDirectTool.jar`)
  - 源码归属: 原项目 `tools/mtk_direct/MtkDirectTool.java`
  - 用途: 远程推送至显示器运行，用于调用 MTK 底层 JNI 寄存器接口与广播画质刷新。
- **ColorfulLedTool.jar** (`assets/runtime/ColorfulLedTool.jar`)
  - 源码归属: 原项目 `tools/colorful_led/ColorfulLedTool.java`
  - 用途: 远程推送至显示器运行，用于调用小米 PM2 屏幕彩色氛围灯 HIDL 接口。
- **AdbGuardian** (`assets/adb_guardian/adbguardian-signed.apk`)
  - 源码归属: `mitv/adb_guardian` (versionCode 5 / versionName 3.2)
  - 用途: 按需安装在显示器 Android 系统的无障碍保活服务，维护无线 ADB 5555 端口常开。
- 详细归属声明文件: `licenses/components/NOTICE-original-tools.txt`

---

## 3. Rust 运行时与编译依赖清单

下表列出了通过 `Cargo.lock` 解析并针对目标平台 `x86_64-pc-windows-msvc` 筛选的全部依赖 crate 及其许可证：

| 依赖库名称 | 版本 | 声明的 SPDX 许可证 | 本地许可证文本相对路径 |
| :--- | :--- | :--- | :--- |
