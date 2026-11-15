param(
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$ExePath,
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CommitSha
)

$ErrorActionPreference = 'Stop'
# A release lookup may intentionally return HTTP 404; classify it explicitly.
$PSNativeCommandUseErrorActionPreference = $false

if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf) -or (Get-Item -LiteralPath $ExePath).Length -eq 0) {
    throw "Release EXE is missing or empty: $ExePath"
}

function Invoke-ReleaseGh {
    param([string[]]$Arguments)
    $response = & gh @Arguments 2>&1
    $commandExitCode = $LASTEXITCODE
    [PSCustomObject]@{
        ExitCode = $commandExitCode
        Text = ($response | Out-String).Trim()
    }
}

$encodedTag = [Uri]::EscapeDataString($Tag)
# A retagged version can leave an older build running. Never publish its EXE.
$tagCommit = Invoke-ReleaseGh -Arguments @('api', "repos/$Repository/commits/$encodedTag", '--jq', '.sha')
if ($tagCommit.ExitCode -ne 0) { throw "Cannot resolve release tag: $($tagCommit.Text)" }
if ($tagCommit.Text -ne $CommitSha) { throw "Tag $Tag moved away from $CommitSha; refusing to publish a stale build" }

$lookup = Invoke-ReleaseGh -Arguments @('api', "repos/$Repository/releases/tags/$encodedTag")
if ($lookup.ExitCode -eq 0) {
    $release = $lookup.Text | ConvertFrom-Json
    if ($release.immutable) { throw "Release $Tag is immutable; its assets cannot be replaced" }
    # Preserve the existing title, notes and publication state; replace only this asset.
    $upload = Invoke-ReleaseGh -Arguments @('release', 'upload', $Tag, $ExePath, '--repo', $Repository, '--clobber')
    if ($upload.ExitCode -ne 0) { throw "Release asset upload failed: $($upload.Text)" }
    $binding = Invoke-ReleaseGh -Arguments @('release', 'edit', $Tag, '--repo', $Repository, '--target', $CommitSha)
    if ($binding.ExitCode -ne 0) { throw "Asset uploaded, but release target update failed: $($binding.Text)" }
    Write-Host "Updated $Tag asset and target $CommitSha"
} elseif ($lookup.Text -match '\(HTTP 404\)') {
    $notes = Join-Path (Split-Path -Parent $PSScriptRoot) ".github/release-notes/$Tag.md"
    $releaseArgs = @('release', 'create', $Tag, $ExePath, '--repo', $Repository, '--verify-tag', '--target', $CommitSha, '--title', $Tag)
    if (Test-Path -LiteralPath $notes -PathType Leaf) {
        $releaseArgs += @('--notes-file', $notes)
    } else {
        $releaseArgs += '--generate-notes'
    }
    $created = Invoke-ReleaseGh -Arguments $releaseArgs
    if ($created.ExitCode -ne 0) { throw "Release creation failed: $($created.Text)" }
    Write-Host $created.Text
} else {
    throw "Release lookup failed; refusing to treat it as a missing release: $($lookup.Text)"
}
