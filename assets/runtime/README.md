# Runtime Assets

ADB and its support DLLs in this directory are embedded into the Rust executable.
The app extracts them to its private versioned runtime directory. The helper
JARs are also embedded and extracted there before being pushed to the display's Android system.

- `adb.exe`: ADB client used by the desktop app.
- `AdbWinApi.dll` and `AdbWinUsbApi.dll`: Windows ADB runtime libraries.
- `MtkDirectTool.jar`: Helper jar pushed to the display for MTK JNI calls.
- `ColorfulLedTool.jar`: Helper jar used to call MiTV PM2 colorful LED HIDL.

Keep these files in this directory so local builds and GitHub Actions use the
same resource layout.

Source for `MtkDirectTool.jar` is kept in `tools/mtk_direct/`.
Source for `ColorfulLedTool.jar` is kept in `tools/colorful_led/`.
