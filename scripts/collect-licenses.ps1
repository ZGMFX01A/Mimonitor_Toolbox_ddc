param(
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$scriptDir = $PSScriptRoot
$projectRoot = Split-Path -Parent $scriptDir

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "  Mimonitor Toolbox 第三方依赖与开源许可归集脚本      " -ForegroundColor Cyan
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

# -----------------------------------------------------------------------------
# 2. 探测真实 Cargo 工具链
# -----------------------------------------------------------------------------
$realCargoPath = $null
$candidateCargoPaths = @()
if ($env:CARGO_HOME) { $candidateCargoPaths += (Join-Path $env:CARGO_HOME "bin\cargo.exe") }
if ($env:RUSTUP_HOME) {
    $candidateCargoPaths += (Join-Path $env:RUSTUP_HOME "toolchains\1.97.1-x86_64-pc-windows-msvc\bin\cargo.exe")
    $candidateCargoPaths += (Join-Path $env:RUSTUP_HOME "toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe")
}
if ($env:USERPROFILE) {
    $candidateCargoPaths += (Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe")
    $candidateCargoPaths += (Join-Path $env:USERPROFILE ".rustup\toolchains\1.97.1-x86_64-pc-windows-msvc\bin\cargo.exe")
    $candidateCargoPaths += (Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe")
}
$cmdCargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
if ($cmdCargo) { $candidateCargoPaths += $cmdCargo.Source }
$candidateCargoPaths += "D:\Rust\cargo\bin\cargo.exe"
$candidateCargoPaths += "D:\Rust\rustup\toolchains\1.97.1-x86_64-pc-windows-msvc\bin\cargo.exe"
$candidateCargoPaths += "D:\Rust\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe"

foreach ($candidate in $candidateCargoPaths) {
    if (Test-ValidBinary $candidate) {
        $realCargoPath = $candidate
        break
    }
}

if (-not $realCargoPath) {
    Write-Host "[错误] 未能找到可用的 cargo.exe！" -ForegroundColor Red
    exit 1
}

$realCargoBinDir = Split-Path -Parent $realCargoPath
$env:PATH = "$realCargoBinDir;$env:PATH"

# -----------------------------------------------------------------------------
# 3. 探测本地 crates.io 源码缓存目录
# -----------------------------------------------------------------------------
Write-Host "[1/4] 探测本地 crates.io 源码缓存..." -ForegroundColor Yellow

$registryRoots = @()
if ($env:CARGO_HOME) { $registryRoots += (Join-Path $env:CARGO_HOME "registry\src") }
if ($env:USERPROFILE) { $registryRoots += (Join-Path $env:USERPROFILE ".cargo\registry\src") }
$registryRoots += "D:\Rust\cargo\registry\src"

$foundRegistrySrcDirs = @()
foreach ($r in $registryRoots) {
    if (Test-Path -LiteralPath $r) {
        $subDirs = Get-ChildItem -LiteralPath $r -Directory -ErrorAction SilentlyContinue
        foreach ($sd in $subDirs) {
            $foundRegistrySrcDirs += $sd.FullName
        }
    }
}

if ($foundRegistrySrcDirs.Count -eq 0) {
    Write-Host "[警告] 未探测到本地 crates.io 源码解压缓存目录。" -ForegroundColor Yellow
} else {
    Write-Host "      找到 crates 缓存库: $($foundRegistrySrcDirs -join '; ')" -ForegroundColor Green
}

# -----------------------------------------------------------------------------
# 4. 执行 cargo metadata 获取精确的依赖包树
# -----------------------------------------------------------------------------
Write-Host "[2/4] 解析 Cargo.lock 与平台依赖元数据..." -ForegroundColor Yellow
Set-Location -LiteralPath $projectRoot

$metaJsonRaw = & $realCargoPath metadata --locked --offline --filter-platform x86_64-pc-windows-msvc --format-version 1 2>&1 | Out-String

if ($LASTEXITCODE -ne 0 -or -not $metaJsonRaw.Trim().StartsWith("{")) {
    Write-Host "[错误] 执行 cargo metadata 失败！" -ForegroundColor Red
    Write-Host $metaJsonRaw -ForegroundColor Red
    exit 1
}

$metaObj = $metaJsonRaw | ConvertFrom-Json
$packages = $metaObj.packages | Where-Object { $_.name -ne "mimonitor_toolbox_ddc" } | Sort-Object name, version

Write-Host "      检测到实际 Windows 平台相关依赖包总数: $($packages.Count)" -ForegroundColor Green

# -----------------------------------------------------------------------------
# 5. 归集依赖许可文件至 licenses/dependencies/
# -----------------------------------------------------------------------------
Write-Host "[3/4] 归集依赖许可文本至 licenses/dependencies/..." -ForegroundColor Yellow

$licensesDir = Join-Path $projectRoot "licenses"
$depOutputDir = Join-Path $licensesDir "dependencies"

if (Test-Path -LiteralPath $depOutputDir) {
    Remove-Item -LiteralPath $depOutputDir -Recurse -Force -ErrorAction Stop
}
New-Item -ItemType Directory -Path $depOutputDir -Force | Out-Null

$results = @()
$missingFilePackages = @()

$utf8BomEncoding = New-Object System.Text.UTF8Encoding($true)

foreach ($pkg in $packages) {
    $pkgName = $pkg.name
    $pkgVersion = $pkg.version
    $pkgSpdx = if ($pkg.license) { $pkg.license } else { "未在元数据中指定" }
    $pkgId = "$pkgName-$pkgVersion"
    $targetPkgDir = Join-Path $depOutputDir $pkgId

    $crateSourceDir = $null
    foreach ($regDir in $foundRegistrySrcDirs) {
        $candidateDir = Join-Path $regDir $pkgId
        if (Test-Path -LiteralPath $candidateDir) {
            $crateSourceDir = $candidateDir
            break
        }
    }

    $copiedFiles = @()
    if ($crateSourceDir) {
        $licenseFiles = Get-ChildItem -LiteralPath $crateSourceDir -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match "^(LICENSE|COPYING|NOTICE|UNLICENSE)" }

        if ($licenseFiles -and $licenseFiles.Count -gt 0) {
            if (-not (Test-Path -LiteralPath $targetPkgDir)) {
                New-Item -ItemType Directory -Path $targetPkgDir -Force | Out-Null
            }
            foreach ($lf in $licenseFiles) {
                $destFile = Join-Path $targetPkgDir $lf.Name
                Copy-Item -LiteralPath $lf.FullName -Destination $destFile -Force
                $copiedFiles += $lf.Name
            }
        }
    }

    # 检查 licenses/supplemental 中是否有已核验补充的官方仓库许可证
    $supplementalPkgDir = Join-Path $licensesDir "supplemental\$pkgId"
    $isUpstreamMissing = $false
    if ($copiedFiles.Count -eq 0 -and (Test-Path -LiteralPath $supplementalPkgDir)) {
        if (-not (Test-Path -LiteralPath $targetPkgDir)) {
            New-Item -ItemType Directory -Path $targetPkgDir -Force | Out-Null
        }
        $suppFiles = Get-ChildItem -LiteralPath $supplementalPkgDir -File -ErrorAction SilentlyContinue
        foreach ($sf in $suppFiles) {
            Copy-Item -LiteralPath $sf.FullName -Destination (Join-Path $targetPkgDir $sf.Name) -Force
            if ($sf.Name -eq "NO_UPSTREAM_LICENSE_FILE.txt") {
                $isUpstreamMissing = $true
            } else {
                $copiedFiles += $sf.Name
            }
        }
    }

    if ($copiedFiles.Count -eq 0) {
        if (-not (Test-Path -LiteralPath $targetPkgDir)) {
            New-Item -ItemType Directory -Path $targetPkgDir -Force | Out-Null
        }
        $upstreamRepo = $pkg.repository
        if (-not $upstreamRepo -and $pkg.name -eq "zune-inflate") {
            $upstreamRepo = "https://github.com/etemesi254/zune-image"
        }
        if (-not $upstreamRepo) {
            $upstreamRepo = "未在 Cargo.toml 登记上游仓库"
        }

        $noticeFileName = if ($isUpstreamMissing) { "NO_UPSTREAM_LICENSE_FILE.txt" } else { "NO_LICENSE_FILE_IN_CRATE.txt" }
        $noticeRelPath = "licenses/dependencies/$pkgId/$noticeFileName"

        if (-not $isUpstreamMissing) {
            $infoFile = Join-Path $targetPkgDir $noticeFileName
            $infoLines = @(
                "Package: $pkgName",
                "Version: $pkgVersion",
                "Declared SPDX License: $pkgSpdx",
                "Upstream Repository: $upstreamRepo",
                "Status: [缺项记录] 该 crate 在发布至 crates.io 时未随包附带独立 LICENSE/NOTICE 文本文件。",
                "License Terms: 其开源许可依据 Cargo.toml package.license 元数据及对应上游开源代码仓库生效。",
                "Notice: 本记录仅如实反映发布包文件缺项现状，严禁虚构版权人与许可证文本。"
            )
            $infoContent = $infoLines -join "`r`n"
            [System.IO.File]::WriteAllText($infoFile, $infoContent, $utf8BomEncoding)
        }

        $missingFilePackages += [PSCustomObject]@{
            Name = $pkgName
            Version = $pkgVersion
            SPDX = $pkgSpdx
            Repository = $upstreamRepo
            NoticePath = $noticeRelPath
        }
    }

    $results += [PSCustomObject]@{
        Name = $pkgName
        Version = $pkgVersion
        SPDX = $pkgSpdx
        CopiedFiles = ($copiedFiles -join ", ")
        HasFile = ($copiedFiles.Count -gt 0)
        RelativeDir = "licenses/dependencies/$pkgId"
    }
}

$totalWithFiles = ($results | Where-Object { $_.HasFile }).Count
Write-Host "      归集完成: 总依赖 $totalWithFiles/$($packages.Count) 个具有完整独立许可文件 (包含已核验的官方仓库补充)，其余 $($missingFilePackages.Count) 个已如实标注上游缺项与元数据声明。" -ForegroundColor Green

# -----------------------------------------------------------------------------
# 6. 生成全局第三方许可通知 THIRD_PARTY_NOTICES.md
# -----------------------------------------------------------------------------
Write-Host "[4/4] 生成项目全局 THIRD_PARTY_NOTICES.md..." -ForegroundColor Yellow

$noticesPath = Join-Path $projectRoot "THIRD_PARTY_NOTICES.md"
$templatesDir = Join-Path $licensesDir "templates"

$headerPath = Join-Path $templatesDir "header.md"
$middlePath = Join-Path $templatesDir "middle.md"
$footerPath = Join-Path $templatesDir "footer.md"

$headerContent = [System.IO.File]::ReadAllText($headerPath, [System.Text.Encoding]::UTF8)
$middleContent = [System.IO.File]::ReadAllText($middlePath, [System.Text.Encoding]::UTF8)
$footerContent = [System.IO.File]::ReadAllText($footerPath, [System.Text.Encoding]::UTF8)

$tableLines = [System.Collections.Generic.List[string]]::new()
foreach ($item in $results) {
    $loc = if ($item.HasFile) {
        '`' + $item.RelativeDir + '`'
    } else {
        '*(未取得独立许可文本，见第4节)*'
    }
    $row = '| **' + $item.Name + '** | ' + $item.Version + ' | ' + $item.SPDX + ' | ' + $loc + ' |'
    $tableLines.Add($row)
}

$missingLines = [System.Collections.Generic.List[string]]::new()
foreach ($m in $missingFilePackages) {
    $repoLink = if ($m.Repository.StartsWith("http")) { "[" + $m.Repository + "](" + $m.Repository + ")" } else { $m.Repository }
    $row = '| **' + $m.Name + '** | ' + $m.Version + ' | ' + $m.SPDX + ' | ' + $repoLink + ' | `' + $m.NoticePath + '` |'
    $missingLines.Add($row)
}

$outputBlocks = @(
    $headerContent.TrimEnd(),
    [string]::Join("`r`n", $tableLines),
    "",
    $middleContent.TrimEnd(),
    [string]::Join("`r`n", $missingLines),
    "",
    $footerContent.TrimEnd()
)

$finalDoc = $outputBlocks -join "`r`n"
[System.IO.File]::WriteAllText($noticesPath, $finalDoc, $utf8BomEncoding)

Write-Host "====================================================" -ForegroundColor Green
Write-Host "  第三方许可证归集与通知生成完毕！                   " -ForegroundColor Green
Write-Host "====================================================" -ForegroundColor Green
exit 0
