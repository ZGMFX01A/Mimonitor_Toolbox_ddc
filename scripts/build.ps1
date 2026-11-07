param(
    [switch]$Check,
    [switch]$Test,
    [switch]$Debug,
    [switch]$Release,
    [switch]$Run,
    [string[]]$CargoArgs
)

# 确保脚本在出错时立即停止，并严格传递失败退出码
$ErrorActionPreference = "Stop"

# 定位工程根目录（基于脚本自身所在目录，不依赖调用时的当前工作目录）
$scriptDir = $PSScriptRoot
$projectRoot = Split-Path -Parent $scriptDir

# Keep compiler and test scratch files in the source repository as well.
$buildTempDir = Join-Path $projectRoot 'target\build-temp'
[void](New-Item -ItemType Directory -Path $buildTempDir -Force)
$env:TEMP = $buildTempDir
$env:TMP = $buildTempDir

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "  Mimonitor Toolbox DDC 构建脚本 (Rust + GPUI)       " -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "工程目录: $projectRoot" -ForegroundColor Gray

# -----------------------------------------------------------------------------
# 1. 辅助函数：校验可执行文件是否有效（非 0 字节损坏占位符）
# -----------------------------------------------------------------------------
function Test-ValidBinary {
    param([string]$FilePath)
    if (-not (Test-Path -LiteralPath $FilePath)) {
        return $false
    }
    try {
        $item = Get-Item -LiteralPath $FilePath -ErrorAction Stop
        if ($item.Length -le 0) {
            return $false
        }
        return $true
    } catch {
        return $false
    }
}

# -----------------------------------------------------------------------------
# 2. 探测真实有效 Rust 工具链 (目标: 严格匹配 Rust 1.97.1, x86_64-pc-windows-msvc)
# -----------------------------------------------------------------------------
Write-Host "[1/4] 探测并校验 Rust 1.97.1 工具链..." -ForegroundColor Yellow

$REQUIRED_RUST_VERSION = "1.97.1"
$realCargoPath = $null
$realRustcPath = $null
$cargoToolchainMethod = $null
$detectedNonMatchingVersions = @()

# 2.1 候选 bin 目录列表（精确 1.97.1 命名工具链优先，标准环境次之，本地路径兜底）
$candidateBinDirs = @()

# 优先探测精确命名的 1.97.1 工具链目录
if ($env:RUSTUP_HOME) {
    $candidateBinDirs += (Join-Path $env:RUSTUP_HOME "toolchains\1.97.1-x86_64-pc-windows-msvc\bin")
}
if ($env:USERPROFILE) {
    $candidateBinDirs += (Join-Path $env:USERPROFILE ".rustup\toolchains\1.97.1-x86_64-pc-windows-msvc\bin")
}
$candidateBinDirs += "D:\Rust\rustup\toolchains\1.97.1-x86_64-pc-windows-msvc\bin"

# 其次探测默认/stable 工具链目录（后续会严格比对其 rustc 真实版本是否为 1.97.1）
if ($env:RUSTUP_HOME) {
    $candidateBinDirs += (Join-Path $env:RUSTUP_HOME "toolchains\stable-x86_64-pc-windows-msvc\bin")
}
if ($env:USERPROFILE) {
    $candidateBinDirs += (Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin")
}
$candidateBinDirs += "D:\Rust\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin"

# 来自 CARGO_HOME / 用户 .cargo / 系统 PATH
if ($env:CARGO_HOME) { $candidateBinDirs += (Join-Path $env:CARGO_HOME "bin") }
if ($env:USERPROFILE) { $candidateBinDirs += (Join-Path $env:USERPROFILE ".cargo\bin") }
$candidateBinDirs += "D:\Rust\cargo\bin"

$cmdCargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
if ($cmdCargo) {
    $candidateBinDirs += (Split-Path -Parent $cmdCargo.Source)
}

# 遍历候选目录，剔除 0 字节损坏占位符并严格校验配套 rustc 版本是否为 1.97.1
foreach ($binDir in $candidateBinDirs) {
    $cargoFile = Join-Path $binDir "cargo.exe"
    $rustcFile = Join-Path $binDir "rustc.exe"

    if ((Test-ValidBinary $cargoFile) -and (Test-ValidBinary $rustcFile)) {
        try {
            $rustcVerOutput = & $rustcFile --version 2>&1 | Out-String
            if ($rustcVerOutput -match "rustc\s+([0-9]+\.[0-9]+\.[0-9]+)") {
                $actualRustcVer = $matches[1]
                if ($actualRustcVer -eq $REQUIRED_RUST_VERSION) {
                    $realCargoPath = $cargoFile
                    $realRustcPath = $rustcFile
                    $cargoToolchainMethod = "direct bin (rustc $actualRustcVer at $binDir)"
                    break
                } else {
                    $detectedNonMatchingVersions += "$binDir (rustc $actualRustcVer)"
                }
            }
        } catch {
            # 继续尝试下一个候选
        }
    }
}

# 2.2 若直接探测未匹配，尝试通过 rustup.exe 精确调度 1.97.1 工具链
if (-not $realCargoPath) {
    $candidateRustupPaths = @()
    if ($env:CARGO_HOME) { $candidateRustupPaths += (Join-Path $env:CARGO_HOME "bin\rustup.exe") }
    if ($env:USERPROFILE) { $candidateRustupPaths += (Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe") }
    $cmdRustup = Get-Command rustup.exe -ErrorAction SilentlyContinue
    if ($cmdRustup) { $candidateRustupPaths += $cmdRustup.Source }
    $candidateRustupPaths += "D:\Rust\cargo\bin\rustup.exe"

    foreach ($rup in $candidateRustupPaths) {
        if (Test-ValidBinary $rup) {
            try {
                $rustupCheck = & $rup run 1.97.1-x86_64-pc-windows-msvc rustc --version 2>&1 | Out-String
                if ($rustupCheck -match "rustc\s+1\.97\.1") {
                    $whichCargo = (& $rup which --toolchain 1.97.1-x86_64-pc-windows-msvc cargo 2>&1 | Out-String).Trim()
                    $whichRustc = (& $rup which --toolchain 1.97.1-x86_64-pc-windows-msvc rustc 2>&1 | Out-String).Trim()
                    if ((Test-ValidBinary $whichCargo) -and (Test-ValidBinary $whichRustc)) {
                        $realCargoPath = $whichCargo
                        $realRustcPath = $whichRustc
                        $cargoToolchainMethod = "rustup 1.97.1 toolchain"
                        break
                    }
                }
            } catch {
                # rustup 中可能尚未安装该工具链
            }
        }
    }
}

# 2.3 若仍未找到 1.97.1，严禁使用非匹配版本冒充锁定，显式拒绝并提示安装命令
if (-not $realCargoPath) {
    Write-Host "====================================================" -ForegroundColor Red
    Write-Host "  [错误] 未检测到配套的 Rust 1.97.1 工具链！         " -ForegroundColor Red
    Write-Host "====================================================" -ForegroundColor Red
    if ($detectedNonMatchingVersions.Count -gt 0) {
        Write-Host "检测到下列其他版本的 Rust 工具链（不满足 1.97.1 锁定要求）:" -ForegroundColor Yellow
        foreach ($nonMatch in $detectedNonMatchingVersions) {
            Write-Host "  - $nonMatch" -ForegroundColor Yellow
        }
    }
    Write-Host "`n请执行以下命令安装官方 1.97.1 工具链后重新运行构建:" -ForegroundColor Cyan
    Write-Host "    rustup toolchain install 1.97.1-x86_64-pc-windows-msvc`n" -ForegroundColor White
    exit 1
}

# 注入真实 1.97.1 工具链所在目录到当前会话 PATH，并固定 RUSTUP_TOOLCHAIN 环境变量
$realCargoBinDir = Split-Path -Parent $realCargoPath
$env:PATH = "$realCargoBinDir;$env:PATH"
$env:RUSTUP_TOOLCHAIN = "1.97.1-x86_64-pc-windows-msvc"
Write-Host "      Rust 工具链验证锁定: $realCargoPath" -ForegroundColor Green
Write-Host "      配套编译器: $realRustcPath [$cargoToolchainMethod]" -ForegroundColor Green

# -----------------------------------------------------------------------------
# 3. 探测 DirectX HLSL 编译器 (fxc.exe) - GPUI Direct3D 着色器编译必需
# -----------------------------------------------------------------------------
Write-Host "[2/4] 探测 DirectX HLSL 编译器 (fxc.exe)..." -ForegroundColor Yellow

$fxcPath = $null
$fxcCmd = Get-Command fxc.exe -ErrorAction SilentlyContinue
if ($fxcCmd -and (Test-ValidBinary $fxcCmd.Source)) {
    $fxcPath = $fxcCmd.Source
} else {
    # 搜索 Windows SDK 目录
    $sdkRoots = @()
    if ($env:WindowsSdkVerBinPath) { $sdkRoots += $env:WindowsSdkVerBinPath }
    if ($env:WindowsSdkDir) { $sdkRoots += (Join-Path $env:WindowsSdkDir "bin") }
    $sdkRoots += "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    $sdkRoots += "$env:ProgramFiles\Windows Kits\10\bin"

    foreach ($root in $sdkRoots) {
        if (Test-Path -LiteralPath $root) {
            # 优先匹配 x64 版本的 fxc.exe
            $fxcCandidate = Get-ChildItem -LiteralPath $root -Recurse -Filter "fxc.exe" -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match "\\x64\\fxc\.exe$" -and $_.Length -gt 0 } |
                Sort-Object FullName -Descending |
                Select-Object -First 1

            if ($fxcCandidate) {
                $fxcPath = $fxcCandidate.FullName
                break
            }
        }
    }
}

if ($fxcPath) {
    $fxcDir = Split-Path -Parent $fxcPath
    $env:PATH = "$fxcDir;$env:PATH"
    Write-Host "      Windows SDK fxc.exe 就绪: $fxcPath" -ForegroundColor Green
} else {
    Write-Host "      [警告] 未在 PATH 或 Windows Kits SDK 中检测到 x64\fxc.exe。" -ForegroundColor Yellow
    Write-Host "      GPUI Direct3D 着色器构建需要 Windows 10/11 SDK 中的 fxc.exe。" -ForegroundColor Yellow
    Write-Host "      若构建报错 'fxc.exe not found'，请通过 Visual Studio Installer 安装 Windows 10/11 SDK。" -ForegroundColor Yellow
}

# -----------------------------------------------------------------------------
# 4. 探测 Visual Studio MSVC C++ 构建环境
# -----------------------------------------------------------------------------
Write-Host "[3/4] 检查 Visual Studio MSVC C++ 构建工具..." -ForegroundColor Yellow

$vswherePath = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$msvcFound = $false
if (Test-Path -LiteralPath $vswherePath) {
    try {
        $vsInstallPath = & $vswherePath -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vsInstallPath) {
            Write-Host "      检测到 Visual Studio 构建环境: $vsInstallPath" -ForegroundColor Green
            $msvcFound = $true
        }
    } catch {
        # 忽略探测异常
    }
}

if (-not $msvcFound) {
    # 宿主机兜底路径检测
    $vsFallbacks = @(
        "D:\Microsoft Visual Studio\18\BuildTools",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools",
        "C:\Program Files\Microsoft Visual Studio\2022\Community"
    )
    foreach ($fb in $vsFallbacks) {
        if (Test-Path -LiteralPath $fb) {
            Write-Host "      检测到 MSVC 候选路径: $fb" -ForegroundColor Green
            $msvcFound = $true
            break
        }
    }
    if (-not $msvcFound) {
        Write-Host "      [提示] 未通过 vswhere 自动探测到 MSVC VC++ 工具链，依赖 rustc 默认链接器发现机制。" -ForegroundColor Yellow
    }
}

# -----------------------------------------------------------------------------
# 5. 执行 Cargo 构建/检查/测试
# -----------------------------------------------------------------------------
Set-Location -LiteralPath $projectRoot

# 决定执行动作
$action = "build"
if ($Check) {
    $action = "check"
} elseif ($Test) {
    $action = "test"
} elseif ($Run) {
    $action = "run"
}

$cargoInvocationArgs = @($action, "--locked")

# 针对不同操作设置默认目标与 profile
if ($action -eq "test") {
    # 默认对主 binary 运行测试，允许通过 CargoArgs 覆盖
    if (-not ($CargoArgs -contains "--bin" -or $CargoArgs -contains "--test" -or $CargoArgs -contains "--tests")) {
        $cargoInvocationArgs += @("--bin", "mimonitor_toolbox_ddc")
    }
} else {
    # 默认指定主二进制目标
    $cargoInvocationArgs += @("--bin", "mimonitor_toolbox_ddc")

    # 默认采用 release profile，若显式传 -Debug 则保持 debug
    if ($Debug) {
        Write-Host "      [模式] Debug 模式构建" -ForegroundColor Cyan
    } else {
        # 默认 Release
        $cargoInvocationArgs += "--release"
        Write-Host "      [模式] Release 优化模式构建" -ForegroundColor Cyan
    }
}

# 附加自定义参数
if ($CargoArgs) {
    $cargoInvocationArgs += $CargoArgs
}

$cmdLineDisplay = $cargoInvocationArgs -join " "
Write-Host "[4/4] 执行 Cargo 命令: $realCargoPath $cmdLineDisplay" -ForegroundColor Cyan

& $realCargoPath @cargoInvocationArgs
$buildExitCode = $LASTEXITCODE

if ($buildExitCode -ne 0) {
    Write-Host "====================================================" -ForegroundColor Red
    Write-Host "  构建失败！退出码: $buildExitCode                   " -ForegroundColor Red
    Write-Host "====================================================" -ForegroundColor Red
    exit $buildExitCode
}

Write-Host "====================================================" -ForegroundColor Green
Write-Host "  构建成功完成！                                    " -ForegroundColor Green
Write-Host "====================================================" -ForegroundColor Green
exit 0
