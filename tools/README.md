# 显示器端辅助工具源码

MtkDirectTool.java 与 ColorfulLedTool.java 从原项目 tools 目录只读复制，保留原作者归属。对应预编译Jar位于assets/runtime，由Rust ADB模块推送至显示器自身Android环境执行，不要求Windows安装Java。未在本次迁移中重新编译这两个Jar。

AdbGuardian APK继承原项目预编译资源，本工作树没有其原始Android工程；归属与版本信息见licenses/components/NOTICE-original-tools.txt。
