param(
    [string]$BinaryPath,
    [switch]$SkipBuild,
    [switch]$NoZip,
    [string]$Version = "0.1.0",
    [string]$CrtDirectory
)

$ErrorActionPreference = "Stop"

$scriptDir = $PSScriptRoot
$projectRoot = Split-Path -Parent $scriptDir

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "  Mimonitor Toolbox 打包脚本 (Portable Release)       " -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "工程目录: $projectRoot" -ForegroundColor Gray

# -----------------------------------------------------------------------------
# 1. 辅助函数：校验可执行文件
# -----------------------------------------------------------------------------
function Test-ValidBinary {
    param([string]$FilePath)
    if (-not (Test-Path -LiteralPath $FilePath)) { return $false }
    try {
        $item = Get-Item -LiteralPath $FilePath -ErrorAction Stop
        return ($item.Length -gt 0)
    } catch {
        return $false
    }
}

function Get-Sha256Hex {
    param([string]$FilePath)
    $stream = [System.IO.File]::OpenRead($FilePath)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        $hashBytes = $sha.ComputeHash($stream)
        return ([System.BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant())
    } finally {
        $stream.Close()
        $stream.Dispose()
    }
}

function Find-VcRuntimeDll {
    param(
        [string]$ExplicitCrtDir
    )

    if ($ExplicitCrtDir) {
        $candidate = $ExplicitCrtDir
        if (Test-Path -LiteralPath $candidate) {
            if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
                $candidate = Join-Path $candidate "vcruntime140.dll"
            }
            if (Test-ValidBinary $candidate) {
                $resolved = (Resolve-Path -LiteralPath $candidate).Path
                if ($resolved -match '(?i)[\\/]System32([\\/]|$)') {
                    Write-Host "[错误] 拒绝使用 System32 下的 vcruntime140.dll！必须使用 Visual Studio 官方 VC++ 可再分发目录。" -ForegroundColor Red
                    return $null
                }
                return $resolved
            }
        }
        Write-Host "[错误] 用户显式指定的 -CrtDirectory ($ExplicitCrtDir) 未找到有效 vcruntime140.dll！" -ForegroundColor Red
        return $null
    }

    # 尝试使用 vswhere 探测 VS 安装目录下的 VC Redist
    $vswhereCandidates = @()
    if ($env:ProgramFiles) { $vswhereCandidates += (Join-Path $env:ProgramFiles "Microsoft Visual Studio\Installer\vswhere.exe") }
    if (${env:ProgramFiles(x86)}) { $vswhereCandidates += (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe") }

    $allVsInstalls = [System.Collections.Generic.List[string]]::new()
    foreach ($vsw in $vswhereCandidates) {
        if (Test-ValidBinary $vsw) {
            try {
                $installs = & $vsw -products * -prerelease -property installationPath 2>$null
                if ($installs) {
                    foreach ($inst in $installs) {
                        if ($inst -and (Test-Path -LiteralPath $inst) -and -not $allVsInstalls.Contains($inst)) {
                            $allVsInstalls.Add($inst)
                        }
                    }
                }
            } catch {}
        }
    }

    # 典型路径回退探测 (包含宿主 BuildTools 路径与 VS 典型路径)
    $fallbackDirs = @(
        "D:\Microsoft Visual Studio\18\BuildTools",
        "C:\Microsoft Visual Studio\18\BuildTools",
        "C:\Program Files\Microsoft Visual Studio\2022\Enterprise",
        "C:\Program Files\Microsoft Visual Studio\2022\Professional",
        "C:\Program Files\Microsoft Visual Studio\2022\Community",
        "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools"
    )
    foreach ($fd in $fallbackDirs) {
        if ((Test-Path -LiteralPath $fd) -and -not $allVsInstalls.Contains($fd)) {
            $allVsInstalls.Add($fd)
        }
    }

    foreach ($inst in $allVsInstalls) {
        $searchGlob = Join-Path $inst "VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll"
        $matched = Get-ChildItem -Path $searchGlob -File -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -notmatch "debug" -and $_.Name -ieq "vcruntime140.dll" -and $_.FullName -notmatch '(?i)[\\/]System32([\\/]|$)' } |
            Sort-Object FullName -Descending
        if ($matched -and $matched.Count -gt 0) {
            return $matched[0].FullName
        }
    }

    # 典型路径回退通配符探测
    $fallbackGlobs = @(
        "D:\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll",
        "C:\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll",
        "C:\Program Files\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll",
        "C:\Program Files (x86)\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT\vcruntime140.dll"
    )
    foreach ($pattern in $fallbackGlobs) {
        $matched = Get-ChildItem -Path $pattern -File -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -notmatch "debug" -and $_.Name -ieq "vcruntime140.dll" -and $_.FullName -notmatch '(?i)[\\/]System32([\\/]|$)' } |
            Sort-Object FullName -Descending
        if ($matched -and $matched.Count -gt 0) {
            return $matched[0].FullName
        }
    }

    return $null
}

# -----------------------------------------------------------------------------
# 2. 获取并核验主执行文件 (默认执行 release 构建)
# -----------------------------------------------------------------------------
$targetExe = $null
$isExternalBinary = $false
$reusedExistingBinary = $false
$builtByScript = $false
$buildProfile = "release"

if ($BinaryPath) {
    Write-Host "[1/5] 使用指定的外部二进制进行打包测试: $BinaryPath" -ForegroundColor Yellow
    if (-not (Test-ValidBinary $BinaryPath)) {
        Write-Host "[错误] 指定的二进制文件不存在或为 0 字节: $BinaryPath" -ForegroundColor Red
        exit 1
    }
    $targetExe = (Resolve-Path -LiteralPath $BinaryPath).Path
    $isExternalBinary = $true
    $reusedExistingBinary = $false
    $builtByScript = $false
    $buildProfile = "external"
} elseif ($SkipBuild) {
    Write-Host "[1/5] 开启 -SkipBuild，尝试寻找已构建的 Release 二进制..." -ForegroundColor Yellow
    $candidateExe = Join-Path $projectRoot "target\release\mimonitor_toolbox_ddc.exe"
    if (-not (Test-ValidBinary $candidateExe)) {
        Write-Host "[错误] 未找到已编译的 target\release\mimonitor_toolbox_ddc.exe，不能跳过构建！" -ForegroundColor Red
        exit 1
    }
    $targetExe = $candidateExe
    $isExternalBinary = $false
    $reusedExistingBinary = $true
    $builtByScript = $false
    $buildProfile = "release_existing"
} else {
    Write-Host "[1/5] 默认调用 scripts/build.ps1 执行 Release 优化构建..." -ForegroundColor Yellow
    $buildScript = Join-Path $scriptDir "build.ps1"
    if (-not (Test-Path -LiteralPath $buildScript)) {
        Write-Host "[错误] 未找到构建脚本: $buildScript" -ForegroundColor Red
        exit 1
    }

    & $buildScript -Release
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[错误] 生产 Release 构建失败！" -ForegroundColor Red
        exit $LASTEXITCODE
    }

    $candidateExe = Join-Path $projectRoot "target\release\mimonitor_toolbox_ddc.exe"
    if (-not (Test-ValidBinary $candidateExe)) {
        Write-Host "[错误] 构建脚本虽然退出码为 0，但未生成有效的 Release EXE: $candidateExe" -ForegroundColor Red
        exit 1
    }
    $targetExe = $candidateExe
    $isExternalBinary = $false
    $reusedExistingBinary = $false
    $builtByScript = $true
}

$exeItem = Get-Item -LiteralPath $targetExe
Write-Host "      主二进制就绪: $targetExe ($([math]::Round($exeItem.Length / 1MB, 2)) MB)" -ForegroundColor Green

# -----------------------------------------------------------------------------
# 3. 核验必需伴随资产 (assets)
# -----------------------------------------------------------------------------
Write-Host "[2/5] 核验必需伴随资源完整性..." -ForegroundColor Yellow

$requiredAssets = @(
    "assets\runtime\adb.exe",
    "assets\runtime\AdbWinApi.dll",
    "assets\runtime\AdbWinUsbApi.dll",
    "assets\runtime\MtkDirectTool.jar",
    "assets\runtime\ColorfulLedTool.jar",
    "assets\adb_guardian\adbguardian-signed.apk",
    "assets\app\icon.ico"
)

$missingAssets = @()
foreach ($assetRel in $requiredAssets) {
    $assetFull = Join-Path $projectRoot $assetRel
    if (-not (Test-ValidBinary $assetFull)) {
        $missingAssets += $assetRel
    }
}

if ($missingAssets.Count -gt 0) {
    Write-Host "[错误] 缺少必需的伴随资产，打包中止！" -ForegroundColor Red
    foreach ($ma in $missingAssets) {
        Write-Host "  - 缺失: $ma" -ForegroundColor Red
    }
    exit 1
}
Write-Host "      所有必需资产核验通过。" -ForegroundColor Green

Write-Host "      探测匹配 x64 的官方 Microsoft VC++ 可再分发运行时库..." -ForegroundColor Yellow
$vcRuntimeDll = Find-VcRuntimeDll -ExplicitCrtDir $CrtDirectory
if (-not $vcRuntimeDll -or -not (Test-ValidBinary $vcRuntimeDll)) {
    Write-Host "[错误] 未能找到匹配 x64 的官方可再分发 vcruntime140.dll！" -ForegroundColor Red
    Write-Host "       本便携包要求 App-Local 分发纯净 CRT 动态库以实现免环境安装运行。" -ForegroundColor Red
    Write-Host "       请安装 Visual Studio 2022+ / Build Tools C++ 桌面开发组件，或通过 -CrtDirectory 参数显式指定路径。" -ForegroundColor Red
    Write-Host "       [安全拦截] 严禁从 C:\Windows\System32 复制系统私有/不可再分发系统 DLL。" -ForegroundColor Red
    exit 1
}
Write-Host "      找到官方 VC++ 可再分发 CRT: $vcRuntimeDll" -ForegroundColor Green

# -----------------------------------------------------------------------------
# 4. 确保开源许可与说明已归集
# -----------------------------------------------------------------------------
Write-Host "[3/5] 确保第三方许可与说明归集就绪..." -ForegroundColor Yellow

$noticesDoc = Join-Path $projectRoot "THIRD_PARTY_NOTICES.md"
$licensesRoot = Join-Path $projectRoot "licenses"

if (-not (Test-Path -LiteralPath $noticesDoc) -or -not (Test-Path -LiteralPath (Join-Path $licensesRoot "dependencies"))) {
    Write-Host "      未检测到完整的许可归集，自动运行 scripts/collect-licenses.ps1..." -ForegroundColor Yellow
    $collectScript = Join-Path $scriptDir "collect-licenses.ps1"
    & $collectScript
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[错误] 依赖许可归集失败！" -ForegroundColor Red
        exit $LASTEXITCODE
    }
}
Write-Host "      开源许可清单与说明就绪。" -ForegroundColor Green

# -----------------------------------------------------------------------------
# 5. 准备输出目录与安全复制
# -----------------------------------------------------------------------------
Write-Host "[4/5] 组装便携包目录结构..." -ForegroundColor Yellow

$distDir = Join-Path $projectRoot "dist"
$portableDir = Join-Path $distDir "Mimonitor_Toolbox_ddc-portable"
$expectedPortableDir = [System.IO.Path]::GetFullPath((Join-Path $distDir "Mimonitor_Toolbox_ddc-portable"))

# 安全核查：严格校验完整规范目录边界，要求完全等于预期的 $projectRoot/dist/Mimonitor_Toolbox_ddc-portable 并拒绝 ReparsePoint (Symlink/Junction) 越界
if (Test-Path -LiteralPath $portableDir) {
    $targetItem = Get-Item -LiteralPath $portableDir -Force
    if ($targetItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        Write-Host "[安全致命错误] 目标输出路径 $portableDir 是重解析点 (ReparsePoint/Junction/Symlink)，拒绝删除！" -ForegroundColor Red
        exit 1
    }
    $actualFullPath = [System.IO.Path]::GetFullPath($targetItem.FullName)
    if ($actualFullPath -ne $expectedPortableDir) {
        Write-Host "[安全致命错误] 目标输出路径规范化结果 ($actualFullPath) 与预期规范路径 ($expectedPortableDir) 不匹配，拒绝删除！" -ForegroundColor Red
        exit 1
    }
    Remove-Item -LiteralPath $portableDir -Recurse -Force -ErrorAction Stop
}

New-Item -ItemType Directory -Path $portableDir -Force | Out-Null

# 5.1 复制并友好重命名主 EXE，并附带 App-Local vcruntime140.dll
$friendlyExeName = "MimonitorToolbox.exe"
$destExe = Join-Path $portableDir $friendlyExeName
Copy-Item -LiteralPath $targetExe -Destination $destExe -Force
Write-Host "      主程序已复制: $friendlyExeName" -ForegroundColor Green

$destRootVcruntime = Join-Path $portableDir "vcruntime140.dll"
Copy-Item -LiteralPath $vcRuntimeDll -Destination $destRootVcruntime -Force

# 5.2 复制 assets 目录（严格排除源码缓存、pdb、mock 等多余文件）
$destAssetsDir = Join-Path $portableDir "assets"
New-Item -ItemType Directory -Path (Join-Path $destAssetsDir "runtime") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $destAssetsDir "adb_guardian") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $destAssetsDir "app") -Force | Out-Null

# 复制 runtime 关键资产与配套 ADB 运行所需的 vcruntime140.dll
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\runtime\MtkDirectTool.jar") -Destination (Join-Path $destAssetsDir "runtime\MtkDirectTool.jar") -Force
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\runtime\ColorfulLedTool.jar") -Destination (Join-Path $destAssetsDir "runtime\ColorfulLedTool.jar") -Force

$vcHash = Get-Sha256Hex $destRootVcruntime
Write-Host "      已集成官方 App-Local vcruntime140.dll (根目录；ADB 私有 runtime 自动复制所需 CRT，SHA256: $vcHash)" -ForegroundColor Green
if (Test-Path (Join-Path $projectRoot "assets\runtime\README.md")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "assets\runtime\README.md") -Destination (Join-Path $destAssetsDir "runtime\README.md") -Force
}

# 复制 adb_guardian 资产
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\adb_guardian\adbguardian-signed.apk") -Destination (Join-Path $destAssetsDir "adb_guardian\adbguardian-signed.apk") -Force
if (Test-Path (Join-Path $projectRoot "assets\adb_guardian\README.md")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "assets\adb_guardian\README.md") -Destination (Join-Path $destAssetsDir "adb_guardian\README.md") -Force
}

# 复制 app 资产
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\app\icon.ico") -Destination (Join-Path $destAssetsDir "app\icon.ico") -Force

# 5.3 复制许可与声明文件
$vsRedistCandidate = "D:\Microsoft Visual Studio\18\BuildTools\Licenses\2052\Redist.txt"
$compRedistDest = Join-Path $licensesRoot "components\Redist.txt"
if ((Test-Path -LiteralPath $vsRedistCandidate) -and (-not (Test-Path -LiteralPath $compRedistDest))) {
    Copy-Item -LiteralPath $vsRedistCandidate -Destination $compRedistDest -Force
}

Copy-Item -LiteralPath $noticesDoc -Destination (Join-Path $portableDir "THIRD_PARTY_NOTICES.md") -Force
$destLicensesDir = Join-Path $portableDir "licenses"
Copy-Item -LiteralPath $licensesRoot -Destination $destLicensesDir -Recurse -Force
$destDocsDir = Join-Path $portableDir "docs"
New-Item -ItemType Directory -Path $destDocsDir -Force | Out-Null
foreach ($docName in @("BUILD.md", "MIGRATION.md", "VALIDATION.md", "USER_FEEDBACK_FIXES.md", "COMMAND_AUDIT.md", "FEATURE_PARITY.md")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "docs\$docName") -Destination (Join-Path $destDocsDir $docName) -Force
}

# 5.4 写入便携包运行说明
$readmeTxt = @"
================================================================================
  Mimonitor Toolbox (红米 G Pro 27U & 多显示器 DDC/CI 工具箱) 便携版
================================================================================

【版本信息】: v$Version (Rust 1.97.1 + 原生 GPUI Windows 客户端)
【系统要求】: Windows 10 / 11 64位 (x86_64)

【快速启动】:
直接双击运行同级目录下的 MimonitorToolbox.exe 即可。

【运行时无依赖特性】:
- 本程序完全由纯 Rust 原生代码与 Win32 API 驱动，借助 GPUI 采用 Direct3D 硬件加速渲染。
- 运行时完全独立，绝不依赖任何 Python、Qt、Node.js、Electron 或 Java 虚拟机环境。
- ADB 与配套 DLL 内嵌在主程序，自动部署到专属 runtime，使用独立端口 TCP 15038；不调用 PATH 或其他软件的 ADB。
- 已附微软官方 VC++ 运行时可再分发库 (vcruntime140.dll，App-Local 部署于根目录；ADB 私有 runtime 自动复制所需 CRT)。本次实际运行环境及未验证范围见 docs/VALIDATION.md。

【配置与数据隔离说明】:
- 独立配置目录: %LOCALAPPDATA%\.mimonitor_toolbox_ddc\config.json
- 本版本与原 Python 版的配置目录 (%LOCALAPPDATA%\.gpro_controller\config.json) 完全隔离。
- 首次运行时若检测到旧版配置，将自动复制并生成 config.bak 备份文件，保留源文件，绝不修改、删除或就地污染旧配置。
- 支持环境变量 MIMONITOR_SETTINGS_PATH 自定义覆盖配置文件路径。

【第三方许可与可再分发说明】:
- 第三方开源软件许可证、版权所有及必要署名详见 THIRD_PARTY_NOTICES.md 与 licenses/ 目录。
- 随附的 vcruntime140.dll 与 licenses/components/Redist.txt 为微软专有可再分发代码 (Microsoft Visual Studio Redistributable Code)，受微软软件许可协议约束，非开源 MIT 软件。
"@
$utf8Bom = New-Object System.Text.UTF8Encoding($true)
[System.IO.File]::WriteAllText((Join-Path $portableDir "README.txt"), $readmeTxt, $utf8Bom)

# -----------------------------------------------------------------------------
# 6. 生成 SHA256 文件清单与 MANIFEST.json 构建元数据
# -----------------------------------------------------------------------------
Write-Host "      计算所有分发文件 SHA256 清单与版本构建元数据..." -ForegroundColor Yellow

$allFiles = Get-ChildItem -LiteralPath $portableDir -Recurse -File | Sort-Object FullName
$fileRecords = [System.Collections.Generic.List[object]]::new()
$shaLines = [System.Collections.Generic.List[string]]::new()

foreach ($f in $allFiles) {
    $relPath = $f.FullName.Substring($portableDir.Length).TrimStart('\', '/')
    $relPathUnix = $relPath.Replace('\', '/')
    $hash = Get-Sha256Hex $f.FullName
    
    $fileRecords.Add(@{
        "path" = $relPathUnix
        "sha256" = $hash
        "size_bytes" = $f.Length
    })
    $shaLines.Add("$hash  $relPathUnix")
}

# 写入 SHA256SUMS.txt
[System.IO.File]::WriteAllText((Join-Path $portableDir "SHA256SUMS.txt"), ($shaLines -join "`r`n"), [System.Text.Encoding]::ASCII)

# 构建 manifest 元数据
$manifestObj = [ordered]@{
    "app_name" = "mimonitor_toolbox_ddc"
    "friendly_name" = "Mimonitor Toolbox"
    "version" = $Version
    "build_profile" = $buildProfile
    "built_by_script" = $builtByScript
    "external_binary" = $isExternalBinary
    "reused_existing_binary" = $reusedExistingBinary
    "runtime_validation" = "not_performed_by_packager"
    "target_platform" = "x86_64-pc-windows-msvc"
    "toolchain" = "rustc 1.97.1"
    "ui_framework" = "GPUI 0.2.2 (crates.io)"
    "package_timestamp_utc" = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    "main_executable" = $friendlyExeName
    "file_count" = $fileRecords.Count
    "files" = $fileRecords
}

$manifestJson = $manifestObj | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $portableDir "MANIFEST.json"), $manifestJson, $utf8Bom)

# -----------------------------------------------------------------------------
# 7. 生成分发 ZIP 压缩包
# -----------------------------------------------------------------------------
$zipFilename = "Mimonitor_Toolbox_ddc-portable-v$Version-windows-x64.zip"
$zipPath = Join-Path $distDir $zipFilename

if (-not $NoZip) {
    Write-Host "[5/5] 创建便携 ZIP 压缩包: $zipFilename..." -ForegroundColor Yellow
    if (Test-Path -LiteralPath $zipPath) {
        Remove-Item -LiteralPath $zipPath -Force
    }
    
    Compress-Archive -Path (Join-Path $portableDir "*") -DestinationPath $zipPath -Force -WarningAction SilentlyContinue
    
    if (-not (Test-Path -LiteralPath $zipPath) -or (Get-Item -LiteralPath $zipPath).Length -le 0) {
        Write-Host "[错误] ZIP 压缩包生成失败或大小为 0！" -ForegroundColor Red
        exit 1
    }
    
    $zipItem = Get-Item -LiteralPath $zipPath
    $zipHash = Get-Sha256Hex $zipPath
    Write-Host "      ZIP 压缩包就绪: $zipPath" -ForegroundColor Green
    Write-Host "      大小: $([math]::Round($zipItem.Length / 1MB, 2)) MB | SHA256: $zipHash" -ForegroundColor Gray
} else {
    Write-Host "[5/5] 跳过生成 ZIP 压缩包 (-NoZip)" -ForegroundColor Yellow
}

Write-Host "====================================================" -ForegroundColor Green
Write-Host "  打包顺利完成！便携目录: $portableDir               " -ForegroundColor Green
Write-Host "====================================================" -ForegroundColor Green
exit 0
