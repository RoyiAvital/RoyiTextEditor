<#
.SYNOPSIS
Builds the mdBook documentation site into target/book.

.DESCRIPTION
Downloads pinned mdBook, mdbook-pagetoc, and mdbook-mermaid binaries into
target/docs-tools when missing or outdated, verifies their SHA-256 hashes,
captures the editor fixture, then builds docs/.
The screenshot build requires the GNU Rust toolchain and MinGW-w64 windres.
The hierarchy in docs/src/index.md is staged as mdBook's SUMMARY.md and removed
after the build. Live preview keeps the generated hierarchy synchronized.
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
$DocsIndex = Join-Path $DocsDir 'src\index.md'
$DocsSummary = Join-Path $DocsDir 'src\SUMMARY.md'

$SyncSummary = {
    param([string] $IndexPath, [string] $SummaryPath, [bool] $Watch)

    $ErrorActionPreference = 'Stop'
    $Watcher = $null
    try {
        if ($Watch) {
            $Watcher = New-Object IO.FileSystemWatcher
            $Watcher.Path = Split-Path -Parent $IndexPath
            $Watcher.Filter = Split-Path -Leaf $IndexPath
            $Watcher.NotifyFilter = [IO.NotifyFilters]::FileName -bor
                [IO.NotifyFilters]::LastWrite -bor [IO.NotifyFilters]::Size
        }
        do {
            $Text = [IO.File]::ReadAllText($IndexPath)
            if (-not [IO.File]::Exists($SummaryPath) -or
                [IO.File]::ReadAllText($SummaryPath) -cne $Text) {
                [IO.File]::WriteAllText($SummaryPath, $Text, (New-Object Text.UTF8Encoding $false))
            }
            if ($Watch) {
                $null = $Watcher.WaitForChanged([IO.WatcherChangeTypes]::All, 1000)
            }
        } while ($Watch)
    }
    finally {
        if ($Watcher) { $Watcher.Dispose() }
    }
}

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

$MdBookMermaid = @{
    Version = '0.17.1'
    Url     = 'https://github.com/badboy/mdbook-mermaid/releases/download/v0.17.1/mdbook-mermaid-v0.17.1-x86_64-pc-windows-msvc.zip'
    Sha256  = 'BB1E3D99D06E799D4A95A7E6BEB6E8E041BA2C3B1528185E873F3F751E52FEB5'
    Exe     = 'mdbook-mermaid.exe'
}

$Tools = @($MdBook, $MdBookPagetoc, $MdBookMermaid)

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

$StagedFiles = New-Object System.Collections.Generic.List[string]
$CreatedAssetsDir = $false
$SourceAssetsDir = Join-Path $DocsDir 'src\assets'
$MermaidStage = Join-Path $ToolsDir "mermaid-stage-$PID"
$PreviousSiteUrl = [Environment]::GetEnvironmentVariable('MDBOOK_OUTPUT__HTML__SITE_URL', 'Process')
$SummaryJob = $null
try {
    if (Test-Path $DocsSummary) {
        throw "Refusing to overwrite an existing generated hierarchy: $DocsSummary"
    }
    $StagedFiles.Add($DocsSummary)
    & $SyncSummary $DocsIndex $DocsSummary $false
    if ($Serve) {
        $SummaryJob = Start-Job -ScriptBlock $SyncSummary -ArgumentList $DocsIndex, $DocsSummary, $true
    }

    $CaptureScript = Join-Path $PSScriptRoot 'Capture-DocsScreenshot.ps1'
    $ImageCache = & $CaptureScript | Select-Object -Last 1
    if (-not $ImageCache -or -not (Test-Path $ImageCache)) {
        throw 'Screenshot helper did not produce an image.'
    }

    if (-not (Test-Path $SourceAssetsDir)) {
        New-Item -ItemType Directory -Path $SourceAssetsDir | Out-Null
        $CreatedAssetsDir = $true
    }
    $SourceImage = Join-Path $SourceAssetsDir 'editor-screenshot.png'
    if (Test-Path $SourceImage) {
        throw "Refusing to overwrite existing file: $SourceImage"
    }
    Copy-Item $ImageCache $SourceImage
    $StagedFiles.Add($SourceImage)

    New-Item -ItemType Directory -Path $MermaidStage | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $MermaidStage 'book.toml'),
        "[output.html]`nadditional-js = []`n",
        (New-Object Text.UTF8Encoding $false)
    )
    & (Join-Path $ToolsDir $MdBookMermaid.Exe) install $MermaidStage
    if ($LASTEXITCODE -ne 0) {
        throw "mdbook-mermaid install failed with exit code $LASTEXITCODE."
    }
    foreach ($File in @('mermaid.min.js', 'mermaid-init.js')) {
        $InstalledFile = Get-ChildItem -Path $MermaidStage -Filter $File -File -Recurse | Select-Object -First 1
        if (-not $InstalledFile) {
            throw "mdbook-mermaid did not install $File."
        }
        $Destination = Join-Path $DocsDir "theme\$File"
        if (Test-Path $Destination) {
            throw "Refusing to overwrite existing file: $Destination"
        }
        Copy-Item $InstalledFile.FullName $Destination
        $StagedFiles.Add($Destination)
    }

    if ($Serve) {
        $env:MDBOOK_OUTPUT__HTML__SITE_URL = '/'
        & mdbook serve $DocsDir --hostname localhost --port 3000 --open
        if ($LASTEXITCODE -ne 0) {
            throw "mdbook serve failed with exit code $LASTEXITCODE"
        }
    } else {
        if (Test-Path $BookDir) {
            Remove-Item -Recurse -Force $BookDir
        }

        $ErrorActionPreference = 'Continue'
        $Output = @(& mdbook build $DocsDir 2>&1 | ForEach-Object { "$_" })
        $ExitCode = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'

        $Output | ForEach-Object { Write-Host $_ }
        if ($ExitCode -ne 0) {
            throw "mdbook build failed with exit code $ExitCode"
        }
        $Problems = @($Output | Where-Object {
            $_ -match '\b(WARN|ERROR)\b|Warning:' -and
            $_ -notmatch 'mdbook-mermaid preprocessor was built against version 0\.5\.0 of mdbook'
        })
        if ($Problems.Count -gt 0) {
            throw "mdbook reported $($Problems.Count) warning(s) or error(s)"
        }
        foreach ($OutputFile in @(
            (Join-Path $BookDir 'index.html'),
            (Join-Path $BookDir 'assets\editor-screenshot.png')
        )) {
            if (-not (Test-Path $OutputFile)) {
                throw "mdbook did not create $OutputFile"
            }
        }
        $MermaidOutputFiles = @(Get-ChildItem -Path (Join-Path $BookDir 'theme') -Filter 'mermaid*.js' -File)
        if ($MermaidOutputFiles.Count -lt 2) {
            throw 'mdbook did not create both fingerprinted Mermaid scripts.'
        }
        Write-Host "Documentation build complete: $BookDir"
    }
}
finally {
    if ($SummaryJob) {
        Stop-Job $SummaryJob -ErrorAction Continue
        Receive-Job $SummaryJob -ErrorAction Continue
        Remove-Job $SummaryJob -ErrorAction Continue
    }
    foreach ($File in $StagedFiles) {
        Remove-Item $File -Force -ErrorAction SilentlyContinue
    }
    if ($CreatedAssetsDir -and (Test-Path $SourceAssetsDir) -and
        -not (Get-ChildItem $SourceAssetsDir -Force | Select-Object -First 1)) {
        Remove-Item $SourceAssetsDir -Force
    }
    if (Test-Path $MermaidStage) {
        Remove-Item $MermaidStage -Recurse -Force
    }
    [Environment]::SetEnvironmentVariable('MDBOOK_OUTPUT__HTML__SITE_URL', $PreviousSiteUrl, 'Process')
}
