<#
Builds release bundles (game + map editor + assets) and publishes them as a GitHub release.

  scripts\release.ps1                     build Windows and Linux bundles, upload both
  scripts\release.ps1 -Only windows       build just one platform (windows | linux)
  scripts\release.ps1 -NoUpload           build only, leave the archives in dist\
  scripts\release.ps1 -Tag v0.3.0         release tag (default: v<version from Cargo.toml>)
  scripts\release.ps1 -Draft              create the GitHub release as a draft

Requirements: Rust (rustup). The Linux bundle is built inside WSL (it needs Rust installed there too);
without WSL it is skipped with a warning. For the upload: GitHub CLI `gh`, logged in, and the commit pushed.
#>
param(
    [ValidateSet("", "windows", "linux")] [string]$Only = "",
    [switch]$NoUpload,
    [string]$Tag = "",
    [switch]$Draft
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $Tag) { $Tag = "v$version" }
New-Item -ItemType Directory -Force dist | Out-Null
$files = @()

if ($Only -eq "" -or $Only -eq "windows") {
    Write-Host "== building Windows x86_64"
    cargo build --release --bins
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $name = "longrange-$version-windows-x86_64"
    $dir = Join-Path "dist" $name
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    if (Test-Path "$dir.zip") { Remove-Item -Force "$dir.zip" }
    New-Item -ItemType Directory $dir | Out-Null
    Copy-Item target\release\longrange.exe, target\release\lr-editor.exe, README.md, CHANGELOG.md $dir
    if (Test-Path LICENSE) { Copy-Item LICENSE $dir }
    Copy-Item -Recurse assets (Join-Path $dir "assets")
    Compress-Archive -Path $dir -DestinationPath "$dir.zip"
    $files += "$dir.zip"
    Write-Host "bundled $dir.zip"
}
if ($Only -eq "" -or $Only -eq "linux") {
    if (Get-Command wsl -ErrorAction SilentlyContinue) {
        Write-Host "== building Linux x86_64 inside WSL"
        wsl bash -lc "scripts/release.sh --only linux --no-upload"
        if ($LASTEXITCODE -ne 0) { throw "Linux build in WSL failed" }
        $files += "dist\longrange-$version-linux-x86_64.tar.gz"
    } elseif ($Only -eq "linux") {
        throw "The Linux bundle needs WSL (with Rust installed inside it)."
    } else {
        Write-Warning "WSL not found: skipping the Linux bundle. Build it on Linux with scripts/release.sh --only linux."
    }
}

if ($NoUpload) {
    Write-Host "skipping upload (-NoUpload). Archives: $files"
    exit 0
}
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "GitHub CLI 'gh' not found; rerun with -NoUpload or install it" }
if (git status --porcelain) { throw "Uncommitted changes: commit and push first, or rerun with -NoUpload." }
$sha = git rev-parse HEAD
git fetch -q origin
if (-not (git branch -r --contains $sha)) { throw "Commit $sha is not on the remote yet. Push it first, or rerun with -NoUpload." }
gh release view $Tag *> $null
if ($LASTEXITCODE -eq 0) {
    Write-Host "== release $Tag exists, replacing its files"
    gh release upload $Tag @files --clobber
} else {
    Write-Host "== creating release $Tag"
    $extra = @()
    if ($Draft) { $extra += "--draft" }
    # Release text: the section for this tag in CHANGELOG.md, or GitHub's generated notes without one.
    $notes = @(); $on = $false
    foreach ($line in Get-Content CHANGELOG.md) {
        if ($line -eq "## $Tag") { $on = $true; continue }
        if ($line -like "## *") { $on = $false }
        if ($on) { $notes += $line }
    }
    if (($notes -join "").Trim()) {
        $notes | Set-Content dist\release-notes.md -Encoding utf8
        gh release create $Tag @files --target $sha --title "Long Range $Tag" --notes-file dist\release-notes.md @extra
    } else {
        Write-Host "no '## $Tag' section in CHANGELOG.md, using generated notes"
        gh release create $Tag @files --target $sha --title "Long Range $Tag" --generate-notes @extra
    }
}
gh release view $Tag --json url --jq .url
