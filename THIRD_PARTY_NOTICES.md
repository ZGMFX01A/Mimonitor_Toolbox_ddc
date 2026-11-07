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
- **使用范围与说明**: 内嵌未修改的原生 Windows x64 ADB 客户端，用于通过独立隔离端口（TCP 15038）与小米/红米显示器建立通信。

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
| **adler2** | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | `licenses/dependencies/adler2-2.0.1` |
| **aho-corasick** | 1.1.5 | Unlicense OR MIT | `licenses/dependencies/aho-corasick-1.1.5` |
| **aligned** | 0.4.3 | MIT OR Apache-2.0 | `licenses/dependencies/aligned-0.4.3` |
| **aligned-vec** | 0.6.4 | MIT | `licenses/dependencies/aligned-vec-0.6.4` |
| **anyhow** | 1.0.104 | MIT OR Apache-2.0 | `licenses/dependencies/anyhow-1.0.104` |
| **ar_archive_writer** | 0.5.3 | Apache-2.0 WITH LLVM-exception | `licenses/dependencies/ar_archive_writer-0.5.3` |
| **arg_enum_proc_macro** | 0.3.4 | MIT | `licenses/dependencies/arg_enum_proc_macro-0.3.4` |
| **arrayref** | 0.3.9 | BSD-2-Clause | `licenses/dependencies/arrayref-0.3.9` |
| **arrayvec** | 0.7.8 | MIT OR Apache-2.0 | `licenses/dependencies/arrayvec-0.7.8` |
| **as-slice** | 0.2.1 | MIT OR Apache-2.0 | `licenses/dependencies/as-slice-0.2.1` |
| **async_zip** | 0.0.17 | MIT | `licenses/dependencies/async_zip-0.0.17` |
| **async-channel** | 1.9.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-channel-1.9.0` |
| **async-channel** | 2.5.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-channel-2.5.0` |
| **async-compression** | 0.4.52 | MIT OR Apache-2.0 | `licenses/dependencies/async-compression-0.4.52` |
| **async-executor** | 1.14.1 | Apache-2.0 OR MIT | `licenses/dependencies/async-executor-1.14.1` |
| **async-fs** | 2.2.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-fs-2.2.0` |
| **async-global-executor** | 2.4.1 | Apache-2.0 OR MIT | `licenses/dependencies/async-global-executor-2.4.1` |
| **async-io** | 2.6.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-io-2.6.0` |
| **async-lock** | 3.5.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-lock-3.5.0` |
| **async-net** | 2.0.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-net-2.0.0` |
| **async-process** | 2.5.0 | Apache-2.0 OR MIT | `licenses/dependencies/async-process-2.5.0` |
| **async-std** | 1.13.2 | Apache-2.0 OR MIT | `licenses/dependencies/async-std-1.13.2` |
| **async-task** | 4.7.1 | Apache-2.0 OR MIT | `licenses/dependencies/async-task-4.7.1` |
| **atomic** | 0.5.3 | Apache-2.0/MIT | `licenses/dependencies/atomic-0.5.3` |
| **atomic-waker** | 1.1.2 | Apache-2.0 OR MIT | `licenses/dependencies/atomic-waker-1.1.2` |
| **autocfg** | 1.5.1 | Apache-2.0 OR MIT | `licenses/dependencies/autocfg-1.5.1` |
| **av1-grain** | 0.2.5 | BSD-2-Clause | `licenses/dependencies/av1-grain-0.2.5` |
| **avif-serialize** | 0.8.9 | BSD-3-Clause | `licenses/dependencies/avif-serialize-0.8.9` |
| **av-scenechange** | 0.14.1 | MIT | `licenses/dependencies/av-scenechange-0.14.1` |
| **base64** | 0.22.1 | MIT OR Apache-2.0 | `licenses/dependencies/base64-0.22.1` |
| **bit_field** | 0.10.3 | Apache-2.0/MIT | `licenses/dependencies/bit_field-0.10.3` |
| **bitflags** | 1.3.2 | MIT/Apache-2.0 | `licenses/dependencies/bitflags-1.3.2` |
| **bitflags** | 2.13.2 | MIT OR Apache-2.0 | `licenses/dependencies/bitflags-2.13.2` |
| **bitstream-io** | 4.10.0 | MIT/Apache-2.0 | `licenses/dependencies/bitstream-io-4.10.0` |
| **block-buffer** | 0.10.4 | MIT OR Apache-2.0 | `licenses/dependencies/block-buffer-0.10.4` |
| **block-buffer** | 0.12.1 | MIT OR Apache-2.0 | `licenses/dependencies/block-buffer-0.12.1` |
| **blocking** | 1.7.0 | Apache-2.0 OR MIT | `licenses/dependencies/blocking-1.7.0` |
| **bstr** | 1.13.1 | MIT OR Apache-2.0 | `licenses/dependencies/bstr-1.13.1` |
| **built** | 0.8.1 | MIT | `licenses/dependencies/built-0.8.1` |
| **bumpalo** | 3.20.3 | MIT OR Apache-2.0 | `licenses/dependencies/bumpalo-3.20.3` |
| **bytemuck** | 1.25.2 | Zlib OR Apache-2.0 OR MIT | `licenses/dependencies/bytemuck-1.25.2` |
| **byteorder-lite** | 0.1.0 | Unlicense OR MIT | `licenses/dependencies/byteorder-lite-0.1.0` |
| **bytes** | 1.12.1 | MIT | `licenses/dependencies/bytes-1.12.1` |
| **cc** | 1.11.1 | MIT OR Apache-2.0 | `licenses/dependencies/cc-1.11.1` |
| **cfg_aliases** | 0.2.2 | MIT | `licenses/dependencies/cfg_aliases-0.2.2` |
| **cfg-if** | 1.0.5 | MIT OR Apache-2.0 | `licenses/dependencies/cfg-if-1.0.5` |
| **chacha20** | 0.10.2 | MIT OR Apache-2.0 | `licenses/dependencies/chacha20-0.10.2` |
| **chrono** | 0.4.45 | MIT OR Apache-2.0 | `licenses/dependencies/chrono-0.4.45` |
| **color_quant** | 1.1.0 | MIT | `licenses/dependencies/color_quant-1.1.0` |
| **compression-codecs** | 0.4.47 | MIT OR Apache-2.0 | `licenses/dependencies/compression-codecs-0.4.47` |
| **compression-core** | 0.4.34 | MIT OR Apache-2.0 | `licenses/dependencies/compression-core-0.4.34` |
| **concurrent-queue** | 2.5.0 | Apache-2.0 OR MIT | `licenses/dependencies/concurrent-queue-2.5.0` |
| **const-oid** | 0.10.2 | Apache-2.0 OR MIT | `licenses/dependencies/const-oid-0.10.2` |
| **convert_case** | 0.4.0 | MIT | `licenses/dependencies/convert_case-0.4.0` |
| **core_detect** | 1.0.0 | MIT/Apache-2.0 | `licenses/dependencies/core_detect-1.0.0` |
| **core_maths** | 0.1.1 | MIT | `licenses/dependencies/core_maths-0.1.1` |
| **cpufeatures** | 0.2.17 | MIT OR Apache-2.0 | `licenses/dependencies/cpufeatures-0.2.17` |
| **cpufeatures** | 0.3.1 | MIT OR Apache-2.0 | `licenses/dependencies/cpufeatures-0.3.1` |
| **crc32fast** | 1.5.4 | MIT OR Apache-2.0 | `licenses/dependencies/crc32fast-1.5.4` |
| **crossbeam-deque** | 0.8.8 | MIT OR Apache-2.0 | `licenses/dependencies/crossbeam-deque-0.8.8` |
| **crossbeam-epoch** | 0.9.21 | MIT OR Apache-2.0 | `licenses/dependencies/crossbeam-epoch-0.9.21` |
| **crossbeam-queue** | 0.3.14 | MIT OR Apache-2.0 | `licenses/dependencies/crossbeam-queue-0.3.14` |
| **crossbeam-utils** | 0.8.23 | MIT OR Apache-2.0 | `licenses/dependencies/crossbeam-utils-0.8.23` |
| **crypto-common** | 0.1.7 | MIT OR Apache-2.0 | `licenses/dependencies/crypto-common-0.1.7` |
| **crypto-common** | 0.2.2 | MIT OR Apache-2.0 | `licenses/dependencies/crypto-common-0.2.2` |
| **ctor** | 0.4.3 | Apache-2.0 OR MIT | `licenses/dependencies/ctor-0.4.3` |
| **ctor-proc-macro** | 0.0.6 | Apache-2.0 OR MIT | `licenses/dependencies/ctor-proc-macro-0.0.6` |
| **data-url** | 0.3.2 | MIT OR Apache-2.0 | `licenses/dependencies/data-url-0.3.2` |
| **deflate64** | 0.1.12 | MIT | `licenses/dependencies/deflate64-0.1.12` |
| **derive_more** | 0.99.20 | MIT | `licenses/dependencies/derive_more-0.99.20` |
| **digest** | 0.10.7 | MIT OR Apache-2.0 | `licenses/dependencies/digest-0.10.7` |
| **digest** | 0.11.3 | MIT OR Apache-2.0 | `licenses/dependencies/digest-0.11.3` |
| **dirs** | 4.0.0 | MIT OR Apache-2.0 | `licenses/dependencies/dirs-4.0.0` |
| **dirs-sys** | 0.3.7 | MIT OR Apache-2.0 | `licenses/dependencies/dirs-sys-0.3.7` |
| **displaydoc** | 0.2.7 | MIT OR Apache-2.0 | `licenses/dependencies/displaydoc-0.2.7` |
| **dtor** | 0.0.6 | Apache-2.0 OR MIT | `licenses/dependencies/dtor-0.0.6` |
| **dtor-proc-macro** | 0.0.5 | Apache-2.0 OR MIT | `licenses/dependencies/dtor-proc-macro-0.0.5` |
| **dunce** | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | `licenses/dependencies/dunce-1.0.5` |
| **dyn-clone** | 1.0.20 | MIT OR Apache-2.0 | `licenses/dependencies/dyn-clone-1.0.20` |
| **either** | 1.19.0 | MIT OR Apache-2.0 | `licenses/dependencies/either-1.19.0` |
| **embed-resource** | 3.0.12 | MIT | `licenses/dependencies/embed-resource-3.0.12` |
| **encoding_rs** | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause | `licenses/dependencies/encoding_rs-0.8.44` |
| **equator** | 0.4.2 | MIT | `licenses/dependencies/equator-0.4.2` |
| **equator-macro** | 0.4.2 | MIT | `licenses/dependencies/equator-macro-0.4.2` |
| **equivalent** | 1.0.3 | Apache-2.0 OR MIT | `licenses/dependencies/equivalent-1.0.3` |
| **erased-serde** | 0.4.11 | MIT OR Apache-2.0 | `licenses/dependencies/erased-serde-0.4.11` |
| **errno** | 0.3.14 | MIT OR Apache-2.0 | `licenses/dependencies/errno-0.3.14` |
| **etagere** | 0.2.15 | MIT/Apache-2.0 | `licenses/dependencies/etagere-0.2.15` |
| **euclid** | 0.22.14 | MIT OR Apache-2.0 | `licenses/dependencies/euclid-0.22.14` |
| **event-listener** | 2.5.3 | Apache-2.0 OR MIT | `licenses/dependencies/event-listener-2.5.3` |
| **event-listener** | 5.4.2 | Apache-2.0 OR MIT | `licenses/dependencies/event-listener-5.4.2` |
| **event-listener-strategy** | 0.5.4 | Apache-2.0 OR MIT | `licenses/dependencies/event-listener-strategy-0.5.4` |
| **exr** | 1.74.2 | BSD-3-Clause | `licenses/dependencies/exr-1.74.2` |
| **fastrand** | 1.9.0 | Apache-2.0 OR MIT | `licenses/dependencies/fastrand-1.9.0` |
| **fastrand** | 2.5.0 | Apache-2.0 OR MIT | `licenses/dependencies/fastrand-2.5.0` |
| **fax** | 0.2.7 | MIT | `licenses/dependencies/fax-0.2.7` |
| **fdeflate** | 0.3.7 | MIT OR Apache-2.0 | `licenses/dependencies/fdeflate-0.3.7` |
| **filetime** | 0.2.29 | MIT/Apache-2.0 | `licenses/dependencies/filetime-0.2.29` |
| **find-msvc-tools** | 0.1.15 | MIT OR Apache-2.0 | `licenses/dependencies/find-msvc-tools-0.1.15` |
| **flate2** | 1.1.11 | MIT OR Apache-2.0 | `licenses/dependencies/flate2-1.1.11` |
| **float_next_after** | 1.0.0 | MIT | `licenses/dependencies/float_next_after-1.0.0` |
| **float-cmp** | 0.9.0 | MIT | `licenses/dependencies/float-cmp-0.9.0` |
| **flume** | 0.11.1 | Apache-2.0/MIT | `licenses/dependencies/flume-0.11.1` |
| **fnv** | 1.0.7 | Apache-2.0 / MIT | `licenses/dependencies/fnv-1.0.7` |
| **foldhash** | 0.2.0 | Zlib | `licenses/dependencies/foldhash-0.2.0` |
| **fontdb** | 0.23.0 | MIT | `licenses/dependencies/fontdb-0.23.0` |
| **form_urlencoded** | 1.2.2 | MIT OR Apache-2.0 | `licenses/dependencies/form_urlencoded-1.2.2` |
| **futf** | 0.1.5 | MIT / Apache-2.0 | `licenses/dependencies/futf-0.1.5` |
| **futures** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-0.3.34` |
| **futures-channel** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-channel-0.3.34` |
| **futures-core** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-core-0.3.34` |
| **futures-executor** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-executor-0.3.34` |
| **futures-io** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-io-0.3.34` |
| **futures-lite** | 1.13.0 | Apache-2.0 OR MIT | `licenses/dependencies/futures-lite-1.13.0` |
| **futures-lite** | 2.6.1 | Apache-2.0 OR MIT | `licenses/dependencies/futures-lite-2.6.1` |
| **futures-macro** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-macro-0.3.34` |
| **futures-sink** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-sink-0.3.34` |
| **futures-task** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-task-0.3.34` |
| **futures-util** | 0.3.34 | MIT OR Apache-2.0 | `licenses/dependencies/futures-util-0.3.34` |
| **generic-array** | 0.14.7 | MIT | `licenses/dependencies/generic-array-0.14.7` |
| **getrandom** | 0.2.17 | MIT OR Apache-2.0 | `licenses/dependencies/getrandom-0.2.17` |
| **getrandom** | 0.3.4 | MIT OR Apache-2.0 | `licenses/dependencies/getrandom-0.3.4` |
| **getrandom** | 0.4.3 | MIT OR Apache-2.0 | `licenses/dependencies/getrandom-0.4.3` |
| **gif** | 0.14.2 | MIT OR Apache-2.0 | `licenses/dependencies/gif-0.14.2` |
| **globset** | 0.4.21 | Unlicense OR MIT | `licenses/dependencies/globset-0.4.21` |
| **gpui** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui-0.2.2` |
| **gpui_collections** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_collections-0.2.2` |
| **gpui_derive_refineable** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_derive_refineable-0.2.2` |
| **gpui_http_client** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_http_client-0.2.2` |
| **gpui_perf** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_perf-0.2.2` |
| **gpui_refineable** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_refineable-0.2.2` |
| **gpui_semantic_version** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_semantic_version-0.2.2` |
| **gpui_sum_tree** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_sum_tree-0.2.2` |
| **gpui_util** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_util-0.2.2` |
| **gpui_util_macros** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui_util_macros-0.2.2` |
| **gpui-macros** | 0.2.2 | Apache-2.0 | `licenses/dependencies/gpui-macros-0.2.2` |
| **grid** | 0.18.0 | MIT | `licenses/dependencies/grid-0.18.0` |
| **h2** | 0.4.20 | MIT | `licenses/dependencies/h2-0.4.20` |
| **half** | 2.7.1 | MIT OR Apache-2.0 | `licenses/dependencies/half-2.7.1` |
| **hashbrown** | 0.17.1 | MIT OR Apache-2.0 | `licenses/dependencies/hashbrown-0.17.1` |
| **heck** | 0.5.0 | MIT OR Apache-2.0 | `licenses/dependencies/heck-0.5.0` |
| **home** | 0.5.12 | MIT OR Apache-2.0 | `licenses/dependencies/home-0.5.12` |
| **http** | 1.5.0 | MIT OR Apache-2.0 | `licenses/dependencies/http-1.5.0` |
| **httparse** | 1.10.1 | MIT OR Apache-2.0 | `licenses/dependencies/httparse-1.10.1` |
| **http-body** | 1.1.0 | MIT | `licenses/dependencies/http-body-1.1.0` |
| **http-body-util** | 0.1.5 | MIT | `licenses/dependencies/http-body-util-0.1.5` |
| **hybrid-array** | 0.4.15 | MIT OR Apache-2.0 | `licenses/dependencies/hybrid-array-0.4.15` |
| **hyper** | 1.12.0 | MIT | `licenses/dependencies/hyper-1.12.0` |
| **hyper-rustls** | 0.27.10 | Apache-2.0 OR ISC OR MIT | `licenses/dependencies/hyper-rustls-0.27.10` |
| **hyper-util** | 0.1.21 | MIT | `licenses/dependencies/hyper-util-0.1.21` |
| **icu_collections** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_collections-2.4.0` |
| **icu_locale_core** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_locale_core-2.4.0` |
| **icu_normalizer** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_normalizer-2.4.0` |
| **icu_normalizer_data** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_normalizer_data-2.4.0` |
| **icu_properties** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_properties-2.4.0` |
| **icu_properties_data** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_properties_data-2.4.0` |
| **icu_provider** | 2.4.0 | Unicode-3.0 | `licenses/dependencies/icu_provider-2.4.0` |
| **idna** | 1.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/idna-1.1.0` |
| **idna_adapter** | 1.2.2 | Apache-2.0 OR MIT | `licenses/dependencies/idna_adapter-1.2.2` |
| **image** | 0.25.10 | MIT OR Apache-2.0 | `licenses/dependencies/image-0.25.10` |
| **imagesize** | 0.13.0 | MIT | `licenses/dependencies/imagesize-0.13.0` |
| **image-webp** | 0.2.4 | MIT OR Apache-2.0 | `licenses/dependencies/image-webp-0.2.4` |
| **imgref** | 1.12.3 | CC0-1.0 OR Apache-2.0 | `licenses/dependencies/imgref-1.12.3` |
| **indexmap** | 2.14.2 | Apache-2.0 OR MIT | `licenses/dependencies/indexmap-2.14.2` |
| **inventory** | 0.3.25 | MIT OR Apache-2.0 | `licenses/dependencies/inventory-0.3.25` |
| **ipnet** | 2.12.2 | MIT OR Apache-2.0 | `licenses/dependencies/ipnet-2.12.2` |
| **itertools** | 0.14.0 | MIT OR Apache-2.0 | `licenses/dependencies/itertools-0.14.0` |
| **itoa** | 1.0.18 | MIT OR Apache-2.0 | `licenses/dependencies/itoa-1.0.18` |
| **jobserver** | 0.1.35 | MIT OR Apache-2.0 | `licenses/dependencies/jobserver-0.1.35` |
| **kurbo** | 0.11.3 | Apache-2.0 OR MIT | `licenses/dependencies/kurbo-0.11.3` |
| **kv-log-macro** | 1.0.7 | MIT OR Apache-2.0 | `licenses/dependencies/kv-log-macro-1.0.7` |
| **lebe** | 0.5.3 | BSD-3-Clause | `licenses/dependencies/lebe-0.5.3` |
| **libc** | 0.2.191 | MIT OR Apache-2.0 | `licenses/dependencies/libc-0.2.191` |
| **libm** | 0.2.16 | MIT | `licenses/dependencies/libm-0.2.16` |
| **litemap** | 0.8.3 | Unicode-3.0 | `licenses/dependencies/litemap-0.8.3` |
| **lock_api** | 0.4.14 | MIT OR Apache-2.0 | `licenses/dependencies/lock_api-0.4.14` |
| **log** | 0.4.35 | MIT OR Apache-2.0 | `licenses/dependencies/log-0.4.35` |
| **loop9** | 0.1.5 | MIT | `licenses/dependencies/loop9-0.1.5` |
| **lru-slab** | 0.1.3 | MIT OR Apache-2.0 OR Zlib | `licenses/dependencies/lru-slab-0.1.3` |
| **lyon** | 1.0.19 | MIT OR Apache-2.0 | `licenses/dependencies/lyon-1.0.19` |
| **lyon_algorithms** | 1.0.22 | MIT OR Apache-2.0 | `licenses/dependencies/lyon_algorithms-1.0.22` |
| **lyon_geom** | 1.0.20 | MIT OR Apache-2.0 | `licenses/dependencies/lyon_geom-1.0.20` |
| **lyon_path** | 1.0.19 | MIT OR Apache-2.0 | `licenses/dependencies/lyon_path-1.0.19` |
| **lyon_tessellation** | 1.0.23 | MIT OR Apache-2.0 | `licenses/dependencies/lyon_tessellation-1.0.23` |
| **mac** | 0.1.1 | MIT/Apache-2.0 | *(未取得独立许可文本，见第4节)* |
| **maybe-rayon** | 0.1.1 | MIT | `licenses/dependencies/maybe-rayon-0.1.1` |
| **memchr** | 2.8.3 | Unlicense OR MIT | `licenses/dependencies/memchr-2.8.3` |
| **memmap2** | 0.9.12 | MIT OR Apache-2.0 | `licenses/dependencies/memmap2-0.9.12` |
| **mime** | 0.3.18 | MIT OR Apache-2.0 | `licenses/dependencies/mime-0.3.18` |
| **mime_guess** | 2.0.5 | MIT | `licenses/dependencies/mime_guess-2.0.5` |
| **miniz_oxide** | 0.8.9 | MIT OR Zlib OR Apache-2.0 | `licenses/dependencies/miniz_oxide-0.8.9` |
| **miniz_oxide** | 0.9.1 | MIT OR Zlib OR Apache-2.0 | `licenses/dependencies/miniz_oxide-0.9.1` |
| **mio** | 1.2.4 | MIT | `licenses/dependencies/mio-1.2.4` |
| **moxcms** | 0.8.1 | BSD-3-Clause OR Apache-2.0 | `licenses/dependencies/moxcms-0.8.1` |
| **multiversion_no_op** | 1.0.0 | Apache-2.0 OR MIT | `licenses/dependencies/multiversion_no_op-1.0.0` |
| **nanorand** | 0.7.0 | Zlib | `licenses/dependencies/nanorand-0.7.0` |
| **new_debug_unreachable** | 1.0.6 | MIT | `licenses/dependencies/new_debug_unreachable-1.0.6` |
| **no_std_io2** | 0.9.4 | Apache-2.0 OR MIT | `licenses/dependencies/no_std_io2-0.9.4` |
| **nom** | 8.0.0 | MIT | `licenses/dependencies/nom-8.0.0` |
| **noop_proc_macro** | 0.3.0 | MIT | `licenses/dependencies/noop_proc_macro-0.3.0` |
| **num_cpus** | 1.17.0 | MIT OR Apache-2.0 | `licenses/dependencies/num_cpus-1.17.0` |
| **num-bigint** | 0.4.8 | MIT OR Apache-2.0 | `licenses/dependencies/num-bigint-0.4.8` |
| **num-complex** | 0.4.6 | MIT OR Apache-2.0 | `licenses/dependencies/num-complex-0.4.6` |
| **num-derive** | 0.4.2 | MIT OR Apache-2.0 | `licenses/dependencies/num-derive-0.4.2` |
| **num-integer** | 0.1.47 | MIT OR Apache-2.0 | `licenses/dependencies/num-integer-0.1.47` |
| **num-rational** | 0.4.2 | MIT OR Apache-2.0 | `licenses/dependencies/num-rational-0.4.2` |
| **num-traits** | 0.2.19 | MIT OR Apache-2.0 | `licenses/dependencies/num-traits-0.2.19` |
| **object** | 0.39.1 | Apache-2.0 OR MIT | `licenses/dependencies/object-0.39.1` |
| **once_cell** | 1.21.4 | MIT OR Apache-2.0 | `licenses/dependencies/once_cell-1.21.4` |
| **parking** | 2.2.1 | Apache-2.0 OR MIT | `licenses/dependencies/parking-2.2.1` |
| **parking_lot** | 0.12.5 | MIT OR Apache-2.0 | `licenses/dependencies/parking_lot-0.12.5` |
| **parking_lot_core** | 0.9.12 | MIT OR Apache-2.0 | `licenses/dependencies/parking_lot_core-0.9.12` |
| **paste** | 1.0.15 | MIT OR Apache-2.0 | `licenses/dependencies/paste-1.0.15` |
| **pastey** | 0.1.1 | MIT OR Apache-2.0 | `licenses/dependencies/pastey-0.1.1` |
| **percent-encoding** | 2.3.2 | MIT OR Apache-2.0 | `licenses/dependencies/percent-encoding-2.3.2` |
| **pico-args** | 0.5.0 | MIT | `licenses/dependencies/pico-args-0.5.0` |
| **pin-project** | 1.1.13 | Apache-2.0 OR MIT | `licenses/dependencies/pin-project-1.1.13` |
| **pin-project-internal** | 1.1.13 | Apache-2.0 OR MIT | `licenses/dependencies/pin-project-internal-1.1.13` |
| **pin-project-lite** | 0.2.17 | Apache-2.0 OR MIT | `licenses/dependencies/pin-project-lite-0.2.17` |
| **pin-utils** | 0.1.1 | MIT OR Apache-2.0 | `licenses/dependencies/pin-utils-0.1.1` |
| **piper** | 0.2.5 | MIT OR Apache-2.0 | `licenses/dependencies/piper-0.2.5` |
| **png** | 0.17.16 | MIT OR Apache-2.0 | `licenses/dependencies/png-0.17.16` |
| **png** | 0.18.1 | MIT OR Apache-2.0 | `licenses/dependencies/png-0.18.1` |
| **polling** | 3.11.0 | Apache-2.0 OR MIT | `licenses/dependencies/polling-3.11.0` |
| **pollster** | 0.2.5 | Apache-2.0/MIT | `licenses/dependencies/pollster-0.2.5` |
| **postage** | 0.5.0 | MIT | `licenses/dependencies/postage-0.5.0` |
| **potential_utf** | 0.1.6 | Unicode-3.0 | `licenses/dependencies/potential_utf-0.1.6` |
| **ppv-lite86** | 0.2.21 | MIT OR Apache-2.0 | `licenses/dependencies/ppv-lite86-0.2.21` |
| **proc-macro2** | 1.0.107 | MIT OR Apache-2.0 | `licenses/dependencies/proc-macro2-1.0.107` |
| **proc-macro-error2** | 2.0.1 | MIT OR Apache-2.0 | `licenses/dependencies/proc-macro-error2-2.0.1` |
| **proc-macro-error-attr2** | 2.0.0 | MIT OR Apache-2.0 | `licenses/dependencies/proc-macro-error-attr2-2.0.0` |
| **profiling** | 1.0.18 | MIT OR Apache-2.0 | `licenses/dependencies/profiling-1.0.18` |
| **profiling-procmacros** | 1.0.18 | MIT OR Apache-2.0 | `licenses/dependencies/profiling-procmacros-1.0.18` |
| **psm** | 0.1.32 | MIT OR Apache-2.0 | `licenses/dependencies/psm-0.1.32` |
| **pulp** | 0.22.3 | MIT | `licenses/dependencies/pulp-0.22.3` |
| **pulp-wasm-simd-flag** | 0.1.1 | MIT | `licenses/dependencies/pulp-wasm-simd-flag-0.1.1` |
| **pxfm** | 0.1.30 | BSD-3-Clause OR Apache-2.0 | `licenses/dependencies/pxfm-0.1.30` |
| **qoi** | 0.4.1 | MIT/Apache-2.0 | `licenses/dependencies/qoi-0.4.1` |
| **quick-error** | 2.0.1 | MIT/Apache-2.0 | `licenses/dependencies/quick-error-2.0.1` |
| **quinn** | 0.11.12 | MIT OR Apache-2.0 | `licenses/dependencies/quinn-0.11.12` |
| **quinn-proto** | 0.11.19 | MIT OR Apache-2.0 | `licenses/dependencies/quinn-proto-0.11.19` |
| **quinn-udp** | 0.5.16 | MIT OR Apache-2.0 | `licenses/dependencies/quinn-udp-0.5.16` |
| **quote** | 1.0.47 | MIT OR Apache-2.0 | `licenses/dependencies/quote-1.0.47` |
| **rand** | 0.10.3 | MIT OR Apache-2.0 | `licenses/dependencies/rand-0.10.3` |
| **rand** | 0.9.5 | MIT OR Apache-2.0 | `licenses/dependencies/rand-0.9.5` |
| **rand_chacha** | 0.9.0 | MIT OR Apache-2.0 | `licenses/dependencies/rand_chacha-0.9.0` |
| **rand_core** | 0.10.1 | MIT OR Apache-2.0 | `licenses/dependencies/rand_core-0.10.1` |
| **rand_core** | 0.6.4 | MIT OR Apache-2.0 | `licenses/dependencies/rand_core-0.6.4` |
| **rand_core** | 0.9.5 | MIT OR Apache-2.0 | `licenses/dependencies/rand_core-0.9.5` |
| **rand_pcg** | 0.10.2 | MIT OR Apache-2.0 | `licenses/dependencies/rand_pcg-0.10.2` |
| **rav1e** | 0.8.1 | BSD-2-Clause | `licenses/dependencies/rav1e-0.8.1` |
| **ravif** | 0.13.0 | BSD-3-Clause | `licenses/dependencies/ravif-0.13.0` |
| **raw-cpuid** | 11.6.0 | MIT | `licenses/dependencies/raw-cpuid-11.6.0` |
| **raw-window-handle** | 0.6.2 | MIT OR Apache-2.0 OR Zlib | `licenses/dependencies/raw-window-handle-0.6.2` |
| **rayon** | 1.12.0 | MIT OR Apache-2.0 | `licenses/dependencies/rayon-1.12.0` |
| **rayon-core** | 1.13.0 | MIT OR Apache-2.0 | `licenses/dependencies/rayon-core-1.13.0` |
| **reborrow** | 0.5.5 | MIT | `licenses/dependencies/reborrow-0.5.5` |
| **ref-cast** | 1.0.27 | MIT OR Apache-2.0 | `licenses/dependencies/ref-cast-1.0.27` |
| **ref-cast-impl** | 1.0.27 | MIT OR Apache-2.0 | `licenses/dependencies/ref-cast-impl-1.0.27` |
| **regex** | 1.13.1 | MIT OR Apache-2.0 | `licenses/dependencies/regex-1.13.1` |
| **regex-automata** | 0.4.18 | MIT OR Apache-2.0 | `licenses/dependencies/regex-automata-0.4.18` |
| **regex-syntax** | 0.8.11 | MIT OR Apache-2.0 | `licenses/dependencies/regex-syntax-0.8.11` |
| **resvg** | 0.45.1 | Apache-2.0 OR MIT | `licenses/dependencies/resvg-0.45.1` |
| **rgb** | 0.8.53 | MIT | `licenses/dependencies/rgb-0.8.53` |
| **ring** | 0.17.14 | Apache-2.0 AND ISC | `licenses/dependencies/ring-0.17.14` |
| **roxmltree** | 0.20.0 | MIT OR Apache-2.0 | `licenses/dependencies/roxmltree-0.20.0` |
| **rustc_version** | 0.4.1 | MIT OR Apache-2.0 | `licenses/dependencies/rustc_version-0.4.1` |
| **rustc-hash** | 2.1.3 | Apache-2.0 OR MIT | `licenses/dependencies/rustc-hash-2.1.3` |
| **rust-embed** | 8.13.0 | MIT | `licenses/dependencies/rust-embed-8.13.0` |
| **rust-embed-impl** | 8.13.0 | MIT | `licenses/dependencies/rust-embed-impl-8.13.0` |
| **rust-embed-utils** | 8.13.0 | MIT | `licenses/dependencies/rust-embed-utils-8.13.0` |
| **rustix** | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `licenses/dependencies/rustix-1.1.6` |
| **rustls** | 0.23.46 | Apache-2.0 OR ISC OR MIT | `licenses/dependencies/rustls-0.23.46` |
| **rustls-native-certs** | 0.8.5 | Apache-2.0 OR ISC OR MIT | `licenses/dependencies/rustls-native-certs-0.8.5` |
| **rustls-pemfile** | 2.2.0 | Apache-2.0 OR ISC OR MIT | `licenses/dependencies/rustls-pemfile-2.2.0` |
| **rustls-pki-types** | 1.15.1 | MIT OR Apache-2.0 | `licenses/dependencies/rustls-pki-types-1.15.1` |
| **rustls-webpki** | 0.103.16 | ISC | `licenses/dependencies/rustls-webpki-0.103.16` |
| **rustversion** | 1.0.23 | MIT OR Apache-2.0 | `licenses/dependencies/rustversion-1.0.23` |
| **rustybuzz** | 0.20.1 | MIT | `licenses/dependencies/rustybuzz-0.20.1` |
| **ryu** | 1.0.23 | Apache-2.0 OR BSL-1.0 | `licenses/dependencies/ryu-1.0.23` |
| **same-file** | 1.0.6 | Unlicense/MIT | `licenses/dependencies/same-file-1.0.6` |
| **schannel** | 0.1.30 | MIT | `licenses/dependencies/schannel-0.1.30` |
| **schemars** | 1.2.3 | MIT | `licenses/dependencies/schemars-1.2.3` |
| **schemars_derive** | 1.2.3 | MIT | `licenses/dependencies/schemars_derive-1.2.3` |
| **scopeguard** | 1.2.0 | MIT OR Apache-2.0 | `licenses/dependencies/scopeguard-1.2.0` |
| **seahash** | 4.1.0 | MIT | *(未取得独立许可文本，见第4节)* |
| **semver** | 1.0.28 | MIT OR Apache-2.0 | `licenses/dependencies/semver-1.0.28` |
| **serde** | 1.0.229 | MIT OR Apache-2.0 | `licenses/dependencies/serde-1.0.229` |
| **serde_core** | 1.0.229 | MIT OR Apache-2.0 | `licenses/dependencies/serde_core-1.0.229` |
| **serde_derive** | 1.0.229 | MIT OR Apache-2.0 | `licenses/dependencies/serde_derive-1.0.229` |
| **serde_derive_internals** | 0.30.0 | MIT OR Apache-2.0 | `licenses/dependencies/serde_derive_internals-0.30.0` |
| **serde_fmt** | 1.1.0 | Apache-2.0 OR MIT | `licenses/dependencies/serde_fmt-1.1.0` |
| **serde_json** | 1.0.154 | MIT OR Apache-2.0 | `licenses/dependencies/serde_json-1.0.154` |
| **serde_json_lenient** | 0.2.4 | MIT/Apache-2.0 | `licenses/dependencies/serde_json_lenient-0.2.4` |
| **serde_spanned** | 1.1.3 | MIT OR Apache-2.0 | `licenses/dependencies/serde_spanned-1.1.3` |
| **serde_urlencoded** | 0.7.1 | MIT/Apache-2.0 | `licenses/dependencies/serde_urlencoded-0.7.1` |
| **sha1_smol** | 1.0.1 | BSD-3-Clause | `licenses/dependencies/sha1_smol-1.0.1` |
| **sha2** | 0.10.9 | MIT OR Apache-2.0 | `licenses/dependencies/sha2-0.10.9` |
| **sha2** | 0.11.1 | MIT OR Apache-2.0 | `licenses/dependencies/sha2-0.11.1` |
| **shlex** | 1.3.0 | MIT OR Apache-2.0 | `licenses/dependencies/shlex-1.3.0` |
| **shlex** | 2.0.1 | MIT OR Apache-2.0 | `licenses/dependencies/shlex-2.0.1` |
| **simd_helpers** | 0.1.0 | MIT | `licenses/dependencies/simd_helpers-0.1.0` |
| **simd-adler32** | 0.3.10 | MIT | `licenses/dependencies/simd-adler32-0.3.10` |
| **simdutf8** | 0.1.5 | MIT OR Apache-2.0 | `licenses/dependencies/simdutf8-0.1.5` |
| **simplecss** | 0.2.2 | Apache-2.0 OR MIT | `licenses/dependencies/simplecss-0.2.2` |
| **siphasher** | 1.0.4 | MIT OR Apache-2.0 | `licenses/dependencies/siphasher-1.0.4` |
| **slab** | 0.4.12 | MIT | `licenses/dependencies/slab-0.4.12` |
| **slotmap** | 1.1.1 | Zlib | `licenses/dependencies/slotmap-1.1.1` |
| **smallvec** | 1.16.5 | MIT OR Apache-2.0 | `licenses/dependencies/smallvec-1.16.5` |
| **smol** | 2.0.2 | Apache-2.0 OR MIT | `licenses/dependencies/smol-2.0.2` |
| **socket2** | 0.6.5 | MIT OR Apache-2.0 | `licenses/dependencies/socket2-0.6.5` |
| **spin** | 0.9.9 | MIT | `licenses/dependencies/spin-0.9.9` |
| **stable_deref_trait** | 1.2.1 | MIT OR Apache-2.0 | `licenses/dependencies/stable_deref_trait-1.2.1` |
| **stacker** | 0.1.25 | MIT OR Apache-2.0 | `licenses/dependencies/stacker-0.1.25` |
| **stacksafe** | 0.1.4 | Apache-2.0 | `licenses/dependencies/stacksafe-0.1.4` |
| **stacksafe-macro** | 0.1.4 | Apache-2.0 | `licenses/dependencies/stacksafe-macro-0.1.4` |
| **static_assertions** | 1.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/static_assertions-1.1.0` |
| **strict-num** | 0.1.1 | MIT | `licenses/dependencies/strict-num-0.1.1` |
| **strum** | 0.27.2 | MIT | `licenses/dependencies/strum-0.27.2` |
| **strum_macros** | 0.27.2 | MIT | `licenses/dependencies/strum_macros-0.27.2` |
| **subtle** | 2.6.1 | BSD-3-Clause | `licenses/dependencies/subtle-2.6.1` |
| **sval** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval-2.22.0` |
| **sval_buffer** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_buffer-2.22.0` |
| **sval_dynamic** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_dynamic-2.22.0` |
| **sval_fmt** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_fmt-2.22.0` |
| **sval_json** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_json-2.22.0` |
| **sval_nested** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_nested-2.22.0` |
| **sval_ref** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_ref-2.22.0` |
| **sval_serde** | 2.22.0 | Apache-2.0 OR MIT | `licenses/dependencies/sval_serde-2.22.0` |
| **svg_fmt** | 0.4.5 | MIT/Apache-2.0 | `licenses/dependencies/svg_fmt-0.4.5` |
| **svgtypes** | 0.15.3 | Apache-2.0 OR MIT | `licenses/dependencies/svgtypes-0.15.3` |
| **syn** | 2.0.119 | MIT OR Apache-2.0 | `licenses/dependencies/syn-2.0.119` |
| **syn** | 3.0.9 | MIT OR Apache-2.0 | `licenses/dependencies/syn-3.0.9` |
| **sync_wrapper** | 1.0.2 | Apache-2.0 | `licenses/dependencies/sync_wrapper-1.0.2` |
| **synstructure** | 0.14.0 | MIT | `licenses/dependencies/synstructure-0.14.0` |
| **taffy** | 0.9.0 | MIT | `licenses/dependencies/taffy-0.9.0` |
| **take-until** | 0.2.0 | MIT | `licenses/dependencies/take-until-0.2.0` |
| **tempfile** | 3.27.0 | MIT OR Apache-2.0 | `licenses/dependencies/tempfile-3.27.0` |
| **tendril** | 0.4.3 | MIT/Apache-2.0 | `licenses/dependencies/tendril-0.4.3` |
| **thiserror** | 1.0.69 | MIT OR Apache-2.0 | `licenses/dependencies/thiserror-1.0.69` |
| **thiserror** | 2.0.21 | MIT OR Apache-2.0 | `licenses/dependencies/thiserror-2.0.21` |
| **thiserror-impl** | 1.0.69 | MIT OR Apache-2.0 | `licenses/dependencies/thiserror-impl-1.0.69` |
| **thiserror-impl** | 2.0.21 | MIT OR Apache-2.0 | `licenses/dependencies/thiserror-impl-2.0.21` |
| **tiff** | 0.11.3 | MIT | `licenses/dependencies/tiff-0.11.3` |
| **tiny-skia** | 0.11.4 | BSD-3-Clause | `licenses/dependencies/tiny-skia-0.11.4` |
| **tiny-skia-path** | 0.11.4 | BSD-3-Clause | `licenses/dependencies/tiny-skia-path-0.11.4` |
| **tinystr** | 0.8.5 | Unicode-3.0 | `licenses/dependencies/tinystr-0.8.5` |
| **tinyvec** | 1.13.3 | Zlib OR Apache-2.0 OR MIT | `licenses/dependencies/tinyvec-1.13.3` |
| **tokio** | 1.54.1 | MIT | `licenses/dependencies/tokio-1.54.1` |
| **tokio-rustls** | 0.26.6 | MIT OR Apache-2.0 | `licenses/dependencies/tokio-rustls-0.26.6` |
| **tokio-socks** | 0.5.3 | MIT | `licenses/dependencies/tokio-socks-0.5.3` |
| **tokio-util** | 0.7.20 | MIT | `licenses/dependencies/tokio-util-0.7.20` |
| **toml** | 1.2.1+spec-1.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/toml-1.2.1+spec-1.1.0` |
| **toml_datetime** | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/toml_datetime-1.1.3+spec-1.1.0` |
| **toml_parser** | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/toml_parser-1.1.5+spec-1.1.0` |
| **toml_writer** | 1.2.0+spec-1.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/toml_writer-1.2.0+spec-1.1.0` |
| **tower** | 0.5.3 | MIT | `licenses/dependencies/tower-0.5.3` |
| **tower-layer** | 0.3.3 | MIT | `licenses/dependencies/tower-layer-0.3.3` |
| **tower-service** | 0.3.3 | MIT | `licenses/dependencies/tower-service-0.3.3` |
| **tracing** | 0.1.44 | MIT | `licenses/dependencies/tracing-0.1.44` |
| **tracing-attributes** | 0.1.31 | MIT | `licenses/dependencies/tracing-attributes-0.1.31` |
| **tracing-core** | 0.1.36 | MIT | `licenses/dependencies/tracing-core-0.1.36` |
| **try-lock** | 0.2.5 | MIT | `licenses/dependencies/try-lock-0.2.5` |
| **ttf-parser** | 0.25.1 | MIT OR Apache-2.0 | `licenses/dependencies/ttf-parser-0.25.1` |
| **typeid** | 1.0.3 | MIT OR Apache-2.0 | `licenses/dependencies/typeid-1.0.3` |
| **typenum** | 1.20.1 | MIT OR Apache-2.0 | `licenses/dependencies/typenum-1.20.1` |
| **unicase** | 2.10.0 | MIT OR Apache-2.0 | `licenses/dependencies/unicase-2.10.0` |
| **unicode-bidi** | 0.3.18 | MIT OR Apache-2.0 | `licenses/dependencies/unicode-bidi-0.3.18` |
| **unicode-bidi-mirroring** | 0.4.0 | MIT/Apache-2.0 | `licenses/dependencies/unicode-bidi-mirroring-0.4.0` |
| **unicode-ccc** | 0.4.0 | MIT/Apache-2.0 | `licenses/dependencies/unicode-ccc-0.4.0` |
| **unicode-ident** | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | `licenses/dependencies/unicode-ident-1.0.27` |
| **unicode-properties** | 0.1.5 | (MIT OR Apache-2.0) AND Unicode-3.0 | `licenses/dependencies/unicode-properties-0.1.5` |
| **unicode-script** | 0.5.9 | MIT OR Apache-2.0 | `licenses/dependencies/unicode-script-0.5.9` |
| **unicode-segmentation** | 1.13.3 | MIT OR Apache-2.0 | `licenses/dependencies/unicode-segmentation-1.13.3` |
| **unicode-vo** | 0.1.0 | MIT/Apache-2.0 | `licenses/dependencies/unicode-vo-0.1.0` |
| **untrusted** | 0.9.0 | ISC | `licenses/dependencies/untrusted-0.9.0` |
| **url** | 2.5.8 | MIT OR Apache-2.0 | `licenses/dependencies/url-2.5.8` |
| **usvg** | 0.45.1 | Apache-2.0 OR MIT | `licenses/dependencies/usvg-0.45.1` |
| **utf_types** | 1.0.0 | Apache-2.0 OR MIT | `licenses/dependencies/utf_types-1.0.0` |
| **utf-8** | 0.7.6 | MIT OR Apache-2.0 | `licenses/dependencies/utf-8-0.7.6` |
| **utf8_iter** | 1.1.0 | Apache-2.0 OR MIT | `licenses/dependencies/utf8_iter-1.1.0` |
| **uuid** | 1.29.1 | Apache-2.0 OR MIT | `licenses/dependencies/uuid-1.29.1` |
| **v_frame** | 0.3.9 | BSD-2-Clause | `licenses/dependencies/v_frame-0.3.9` |
| **value-bag** | 1.14.1 | Apache-2.0 OR MIT | `licenses/dependencies/value-bag-1.14.1` |
| **value-bag-serde1** | 1.14.1 | Apache-2.0 OR MIT | `licenses/dependencies/value-bag-serde1-1.14.1` |
| **value-bag-sval2** | 1.14.1 | Apache-2.0 OR MIT | `licenses/dependencies/value-bag-sval2-1.14.1` |
| **version_check** | 0.9.5 | MIT/Apache-2.0 | `licenses/dependencies/version_check-0.9.5` |
| **vswhom** | 0.1.0 | MIT | `licenses/dependencies/vswhom-0.1.0` |
| **vswhom-sys** | 0.1.3 | MIT | `licenses/dependencies/vswhom-sys-0.1.3` |
| **waker-fn** | 1.2.0 | Apache-2.0 OR MIT | `licenses/dependencies/waker-fn-1.2.0` |
| **walkdir** | 2.5.0 | Unlicense/MIT | `licenses/dependencies/walkdir-2.5.0` |
| **want** | 0.3.2 | MIT | `licenses/dependencies/want-0.3.2` |
| **wasm-bindgen** | 0.2.130 | MIT OR Apache-2.0 | `licenses/dependencies/wasm-bindgen-0.2.130` |
| **wasm-bindgen-macro** | 0.2.130 | MIT OR Apache-2.0 | `licenses/dependencies/wasm-bindgen-macro-0.2.130` |
| **wasm-bindgen-macro-support** | 0.2.130 | MIT OR Apache-2.0 | `licenses/dependencies/wasm-bindgen-macro-support-0.2.130` |
| **wasm-bindgen-shared** | 0.2.130 | MIT OR Apache-2.0 | `licenses/dependencies/wasm-bindgen-shared-0.2.130` |
| **weezl** | 0.1.12 | MIT OR Apache-2.0 | `licenses/dependencies/weezl-0.1.12` |
| **which** | 6.0.3 | MIT | `licenses/dependencies/which-6.0.3` |
| **winapi** | 0.3.9 | MIT/Apache-2.0 | `licenses/dependencies/winapi-0.3.9` |
| **winapi-util** | 0.1.11 | Unlicense OR MIT | `licenses/dependencies/winapi-util-0.1.11` |
| **windows** | 0.61.3 | MIT OR Apache-2.0 | `licenses/dependencies/windows-0.61.3` |
| **windows_x86_64_msvc** | 0.53.1 | MIT OR Apache-2.0 | `licenses/dependencies/windows_x86_64_msvc-0.53.1` |
| **windows-collections** | 0.2.0 | MIT OR Apache-2.0 | `licenses/dependencies/windows-collections-0.2.0` |
| **windows-core** | 0.61.2 | MIT OR Apache-2.0 | `licenses/dependencies/windows-core-0.61.2` |
| **windows-future** | 0.2.1 | MIT OR Apache-2.0 | `licenses/dependencies/windows-future-0.2.1` |
| **windows-implement** | 0.60.2 | MIT OR Apache-2.0 | `licenses/dependencies/windows-implement-0.60.2` |
| **windows-interface** | 0.59.3 | MIT OR Apache-2.0 | `licenses/dependencies/windows-interface-0.59.3` |
| **windows-link** | 0.1.3 | MIT OR Apache-2.0 | `licenses/dependencies/windows-link-0.1.3` |
| **windows-link** | 0.2.1 | MIT OR Apache-2.0 | `licenses/dependencies/windows-link-0.2.1` |
| **windows-numerics** | 0.2.0 | MIT OR Apache-2.0 | `licenses/dependencies/windows-numerics-0.2.0` |
| **windows-registry** | 0.4.0 | MIT OR Apache-2.0 | `licenses/dependencies/windows-registry-0.4.0` |
| **windows-registry** | 0.5.3 | MIT OR Apache-2.0 | `licenses/dependencies/windows-registry-0.5.3` |
| **windows-result** | 0.3.4 | MIT OR Apache-2.0 | `licenses/dependencies/windows-result-0.3.4` |
| **windows-strings** | 0.3.1 | MIT OR Apache-2.0 | `licenses/dependencies/windows-strings-0.3.1` |
| **windows-strings** | 0.4.2 | MIT OR Apache-2.0 | `licenses/dependencies/windows-strings-0.4.2` |
| **windows-sys** | 0.61.2 | MIT OR Apache-2.0 | `licenses/dependencies/windows-sys-0.61.2` |
| **windows-targets** | 0.53.5 | MIT OR Apache-2.0 | `licenses/dependencies/windows-targets-0.53.5` |
| **windows-threading** | 0.1.0 | MIT OR Apache-2.0 | `licenses/dependencies/windows-threading-0.1.0` |
| **winnow** | 1.0.5 | MIT | `licenses/dependencies/winnow-1.0.5` |
| **winreg** | 0.56.0 | MIT | `licenses/dependencies/winreg-0.56.0` |
| **winsafe** | 0.0.19 | MIT | `licenses/dependencies/winsafe-0.0.19` |
| **writeable** | 0.6.4 | Unicode-3.0 | `licenses/dependencies/writeable-0.6.4` |
| **xmlwriter** | 0.1.0 | MIT | `licenses/dependencies/xmlwriter-0.1.0` |
| **y4m** | 0.8.0 | MIT | `licenses/dependencies/y4m-0.8.0` |
| **yoke** | 0.8.4 | Unicode-3.0 | `licenses/dependencies/yoke-0.8.4` |
| **yoke-derive** | 0.8.4 | Unicode-3.0 | `licenses/dependencies/yoke-derive-0.8.4` |
| **zed-async-tar** | 0.5.0-zed | MIT/Apache-2.0 | `licenses/dependencies/zed-async-tar-0.5.0-zed` |
| **zed-reqwest** | 0.12.15-zed | MIT OR Apache-2.0 | `licenses/dependencies/zed-reqwest-0.12.15-zed` |
| **zerocopy** | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | `licenses/dependencies/zerocopy-0.8.63` |
| **zerocopy-derive** | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | `licenses/dependencies/zerocopy-derive-0.8.63` |
| **zerofrom** | 0.1.8 | Unicode-3.0 | `licenses/dependencies/zerofrom-0.1.8` |
| **zerofrom-derive** | 0.1.8 | Unicode-3.0 | `licenses/dependencies/zerofrom-derive-0.1.8` |
| **zeroize** | 1.9.1 | Apache-2.0 OR MIT | `licenses/dependencies/zeroize-1.9.1` |
| **zeroize_derive** | 1.5.0 | Apache-2.0 OR MIT | `licenses/dependencies/zeroize_derive-1.5.0` |
| **zerotrie** | 0.2.5 | Unicode-3.0 | `licenses/dependencies/zerotrie-0.2.5` |
| **zerovec** | 0.11.9 | Unicode-3.0 | `licenses/dependencies/zerovec-0.11.9` |
| **zerovec-derive** | 0.11.7 | Unicode-3.0 | `licenses/dependencies/zerovec-derive-0.11.7` |
| **zlib-rs** | 0.6.8 | Zlib | `licenses/dependencies/zlib-rs-0.6.8` |
| **zmij** | 1.0.23 | MIT | `licenses/dependencies/zmij-1.0.23` |
| **zune-core** | 0.5.3 | MIT OR Apache-2.0 OR Zlib | `licenses/dependencies/zune-core-0.5.3` |
| **zune-inflate** | 0.2.54 | MIT OR Apache-2.0 OR Zlib | `licenses/dependencies/zune-inflate-0.2.54` |
| **zune-jpeg** | 0.5.15 | MIT OR Apache-2.0 OR Zlib | `licenses/dependencies/zune-jpeg-0.5.15` |

---

## 4. 上游仓库未随包附带独立许可文本的依赖明示 (缺项记录)

根据依赖完整性与上游代码审计，绝大多数 crates.io 发布包未附带独立许可文件的依赖，已从官方仓库对应 tag/分支补充核验并保留完整版权所有行（存放于 `licenses/dependencies/<crate>/` 并附带 `SOURCE.txt`）。
仅下列 2 个依赖在上游官方代码仓库根目录中亦未包含独立的 LICENSE 文本文件。本项目在此如实记录上游缺项现状，严禁虚构版权人与许可证文本。其开源许可严格依照其在 `Cargo.toml` / `README.md` 中登记的 SPDX 表达式与作者声明生效：

| 依赖库名称 | 版本 | 声明的 SPDX 许可证 | 可找到的上游仓库 | 缺项记录说明文件路径 |
| :--- | :--- | :--- | :--- | :--- |
| **mac** | 0.1.1 | MIT/Apache-2.0 | [https://github.com/reem/rust-mac.git](https://github.com/reem/rust-mac.git) | `licenses/dependencies/mac-0.1.1/NO_UPSTREAM_LICENSE_FILE.txt` |
| **seahash** | 4.1.0 | MIT | [https://gitlab.redox-os.org/redox-os/seahash](https://gitlab.redox-os.org/redox-os/seahash) | `licenses/dependencies/seahash-4.1.0/NO_UPSTREAM_LICENSE_FILE.txt` |

---

## 5. 通用开源许可证全文条款

### 5.1 The MIT License
```text
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### 5.2 Apache License, Version 2.0
详见 `licenses/components/LICENSE-gpui-input.txt`。

### 5.3 许可证归集文本检索说明
所有已归集的依赖许可证原始文本完整存放于本仓库的 `licenses/dependencies/` 目录下，并随同便携包一起分发。
