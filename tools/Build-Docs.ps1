<#
.SYNOPSIS
Builds the mdBook documentation site into target/book.

.DESCRIPTION
Downloads pinned mdBook and mdbook-pagetoc binaries into target/docs-tools when
missing or outdated, verifies their SHA-256 hashes, then builds docs/.
Warnings and errors reported by mdBook fail the build.

.PARAMETER Serve
Serve the documentation locally with auto-reload instead of building.

.EXAMPLE
.\tools\Build-Docs.ps1
.\tools\Build-Docs.ps1 -Serve
#>
[CmdletBinding()]
param(
    [switch] $Serve
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Root = Split-Path -Parent $PSScriptRoot
$ToolsDir = Join-Path $Root 'target\docs-tools'
$DocsDir = Join-Path $Root 'docs'
$BookDir = Join-Path $Root 'target\book'

# Pinned versions and SHA-256 hashes from the official GitHub releases
$MdBook = @{
    Version = '0.5.4'
    Url     = 'https://github.com/rust-lang/mdBook/releases/download/v0.5.4/mdbook-v0.5.4-x86_64-pc-windows-msvc.zip'
    Sha256  = '8A6B2421AA522DE06D746871D2B8FFF9C8D71773467E310A4516D2566E7F2DE4'
    Exe     = 'mdbook.exe'
}

$MdBookPagetoc = @{
    Version = '0.3.0'
    Url     = 'https://github.com/slowsage/mdbook-pagetoc/releases/download/v0.3.0/mdbook-pagetoc-v0.3.0-x86_64-pc-windows-gnu.zip'
    Sha256  = '93C7376F022D604B950A73FAE638E851E49ADEC4066107DDCE0D5B334B1A6888'
    Exe     = 'mdbook-pagetoc.exe'
}

$Tools = @($MdBook, $MdBookPagetoc)

if (-not (Test-Path $ToolsDir)) {
    New-Item -ItemType Directory -Path $ToolsDir -Force | Out-Null
}

foreach ($Tool in $Tools) {
    $ExePath = Join-Path $ToolsDir $Tool.Exe
    $VersionFile = Join-Path $ToolsDir "$($Tool.Exe).version"
    $CachedVersion = if (Test-Path $VersionFile) { (Get-Content $VersionFile -Raw).Trim() } else { $null }

    if ((Test-Path $ExePath) -and $CachedVersion -eq $Tool.Version) {
        Write-Host "$($Tool.Exe) v$($Tool.Version) cached"
        continue
    }

    Write-Host "Downloading $($Tool.Exe) v$($Tool.Version)"
    $TempZip = Join-Path $ToolsDir "$($Tool.Exe).zip"
    try {
        Invoke-WebRequest -Uri $Tool.Url -OutFile $TempZip -ErrorAction Stop
    }
    catch {
        Remove-Item $TempZip -ErrorAction SilentlyContinue
        throw "Failed to download $($Tool.Exe): $_"
    }

    $FileHash = (Get-FileHash -Path $TempZip -Algorithm SHA256).Hash
    if ($FileHash -ne $Tool.Sha256) {
        Remove-Item $TempZip
        throw "SHA-256 mismatch for $($Tool.Exe). Expected $($Tool.Sha256), got $FileHash"
    }

    Expand-Archive -Path $TempZip -DestinationPath $ToolsDir -Force
    Remove-Item $TempZip

    if (-not (Test-Path $ExePath)) {
        $FoundExe = Get-ChildItem -Path $ToolsDir -Filter $Tool.Exe -Recurse | Select-Object -First 1
        if (-not $FoundExe) {
            throw "Could not find $($Tool.Exe) in extracted archive"
        }
        Move-Item -Path $FoundExe.FullName -Destination $ExePath -Force
    }

    Set-Content -Path $VersionFile -Value $Tool.Version
    Write-Host "$($Tool.Exe) v$($Tool.Version) installed"
}

$env:PATH = "$ToolsDir;$env:PATH"

$AppMeta = Get-Content (Join-Path $Root 'Meta\AppMeta.json') -Raw | ConvertFrom-Json
$env:MDBOOK_BOOK__TITLE = $AppMeta.name

if ($Serve) {
    & mdbook serve $DocsDir --open
    exit $LASTEXITCODE
}

if (Test-Path $BookDir) {
    Remove-Item -Recurse -Force $BookDir
}

# mdBook writes its log to stderr; keep it as text so every line is shown
$ErrorActionPreference = 'Continue'
$Output = @(& mdbook build $DocsDir 2>&1 | ForEach-Object { "$_" })
$ExitCode = $LASTEXITCODE
$ErrorActionPreference = 'Stop'

$Output | ForEach-Object { Write-Host $_ }

if ($ExitCode -ne 0) {
    throw "mdbook build failed with exit code $ExitCode"
}

$Problems = @($Output | Where-Object { $_ -match '\b(WARN|ERROR)\b|Warning:' })
if ($Problems.Count -gt 0) {
    throw "mdbook reported $($Problems.Count) warning(s) or error(s)"
}

if (-not (Test-Path (Join-Path $BookDir 'index.html'))) {
    throw "mdbook did not create $BookDir\index.html"
}

Write-Host "Documentation build complete: $BookDir"
