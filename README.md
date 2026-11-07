# Mimonitor Toolbox · Rust + GPUI

[GitHub 仓库](https://github.com/ZGMFX01A/Mimonitor_Toolbox_ddc)

Windows 多屏亮度与小米/Redmi 显示器工具箱。业务逻辑使用 Rust，界面使用 GPUI 0.2.2，保留原有十个功能页面。主窗口使用合并标题栏；托盘左键呼出统一亮度弹窗，小米屏通过 ADB 调背光，普通屏通过 Windows DDC/CI 调亮度。

窗口标题为“Mimonitor 工具箱”，显示器名称旁按精确设备实例的 EDID 显示约算尺寸，无法读取则省略。界面支持统一的深色、浅色和跟随系统主题。

便携包解压后双击 `MimonitorToolbox.exe`。请保留整个目录，包含 `assets`、DLL 和许可文件。ADB 与配套 DLL 已内嵌到主程序，首次使用自动部署至应用专属 runtime 目录，禁止回退调用 PATH 或当前目录里的 ADB。Windows 端不需要 Python、Qt、Node.js、Electron 或 Java；Jar 辅助工具仅在显示器自身的 Android 系统中运行。

- [迁移对照表](docs/MIGRATION.md)
- [旧版功能覆盖与遗漏](docs/FEATURE_PARITY.md)
- [设备命令逐项核对](docs/COMMAND_AUDIT.md)
- [测试结果与已知限制](docs/VALIDATION.md)
- [构建及打包](docs/BUILD.md)
- [第三方许可及署名](THIRD_PARTY_NOTICES.md)

原 Python 工作树保留供回退，本项目不覆盖其未提交改动。当前仍有旧版操作流程未完整迁移，详见功能覆盖报告。

首次运行会在旧配置旁创建备份，再迁移到 `%LOCALAPPDATA%\.mimonitor_toolbox_ddc\config.json`。备份失败则以只读模式启动并提示；旧配置保持原样。日志位于同目录下 `logs`。

普通屏不支持 DDC/CI、映射不唯一或断开时会禁止控制。小米屏不会重复生成 DDC 卡片。设备参数未读到时显示未知并禁用依赖该参数的写操作。

ADB 以普通权限启动，仅监听本机；工具箱自行扫描固定 IP，不启用 ADB 的 mDNS、USB 或模拟器发现。本应用新建的 ADB 及其控制台附属进程随退出或异常终止回收；独立端口 15038 已被其他服务占用时明确报错，保留该服务，不复用或终止它。窗口关闭行为选择“最小化到托盘”时应用仍在运行，需通过托盘“退出”结束，或在设置中选择关闭即退出。随包的 VC++ DLL 不需要运行安装程序。

设备页支持纵向滚动；按钮操作有执行遮罩与全局结果提示，参数连续变更合并后下发，并在设备事务之间保留间隔。普通屏仅参与统一亮度控制，HDR / SDR 精密控光仅接受小米 / Redmi 设备身份。工具页的 ADB 保活守护提供自动检测、部署/修复、启动保活和明确状态。
