<#
.SYNOPSIS
Builds the mdBook documentation site into target/book.

.DESCRIPTION
Downloads pinned mdBook, mdbook-pagetoc, and mdbook-mermaid binaries into
target/docs-tools when missing or outdated, verifies their SHA-256 hashes,
captures the editor fixture, then builds docs/.
The screenshot build requires the GNU Rust toolchain and MinGW-w64 windres.
mdBook reads navigation directly from docs/src/SUMMARY.md. Screenshot assets
are regenerated for each build, staged only for mdBook, and restored afterwards.
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
$DocsSummary = Join-Path $DocsDir 'src\SUMMARY.md'

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
$SourceAssetBackups = @{}
$MermaidStage = Join-Path $ToolsDir "mermaid-stage-$PID"
$PreviousSiteUrl = [Environment]::GetEnvironmentVariable('MDBOOK_OUTPUT__HTML__SITE_URL', 'Process')
try {
    if (-not (Test-Path $DocsSummary -PathType Leaf)) {
        throw "mdBook navigation file not found: $DocsSummary"
    }

    $GeneratedImagesDir = Join-Path $Root 'target\docs-assets'
    if (Test-Path $GeneratedImagesDir) {
        Remove-Item $GeneratedImagesDir -Recurse -Force
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
        $SourceAssetBackups[$SourceImage] = [IO.File]::ReadAllBytes($SourceImage)
    } else {
        $StagedFiles.Add($SourceImage)
    }
    Copy-Item $ImageCache $SourceImage -Force

    $SearchImageCache = & $CaptureScript -SearchPanel | Select-Object -Last 1
    if (-not $SearchImageCache -or -not (Test-Path $SearchImageCache)) {
        throw 'Search panel screenshot helper did not produce an image.'
    }
    $SourceSearchImage = Join-Path $SourceAssetsDir 'search-panel.png'
    if (Test-Path $SourceSearchImage) {
        $SourceAssetBackups[$SourceSearchImage] = [IO.File]::ReadAllBytes($SourceSearchImage)
    } else {
        $StagedFiles.Add($SourceSearchImage)
    }
    Copy-Item $SearchImageCache $SourceSearchImage -Force

    $SyntaxImageCache = & $CaptureScript -SyntaxCode | Select-Object -Last 1
    if (-not $SyntaxImageCache -or -not (Test-Path $SyntaxImageCache)) {
        throw 'Syntax screenshot helper did not produce an image.'
    }
    $SourceSyntaxImage = Join-Path $SourceAssetsDir 'syntax-highlighted-code.png'
    if (Test-Path $SourceSyntaxImage) {
        $SourceAssetBackups[$SourceSyntaxImage] = [IO.File]::ReadAllBytes($SourceSyntaxImage)
    } else {
        $StagedFiles.Add($SourceSyntaxImage)
    }
    Copy-Item $SyntaxImageCache $SourceSyntaxImage -Force

    $LanguageImageCache = & $CaptureScript -SyntaxLanguages | Select-Object -Last 1
    if (-not $LanguageImageCache -or -not (Test-Path $LanguageImageCache)) {
        throw 'Syntax language picker screenshot helper did not produce an image.'
    }
    $SourceLanguageImage = Join-Path $SourceAssetsDir 'syntax-language-picker.png'
    if (Test-Path $SourceLanguageImage) {
        $SourceAssetBackups[$SourceLanguageImage] = [IO.File]::ReadAllBytes($SourceLanguageImage)
    } else {
        $StagedFiles.Add($SourceLanguageImage)
    }
    Copy-Item $LanguageImageCache $SourceLanguageImage -Force

    $SpellingImageCache = & $CaptureScript -Spelling | Select-Object -Last 1
    if (-not $SpellingImageCache -or -not (Test-Path $SpellingImageCache)) {
        throw 'Spelling screenshot helper did not produce an image.'
    }
    $SourceSpellingImage = Join-Path $SourceAssetsDir 'spelling.png'
    if (Test-Path $SourceSpellingImage) {
        $SourceAssetBackups[$SourceSpellingImage] = [IO.File]::ReadAllBytes($SourceSpellingImage)
    } else {
        $StagedFiles.Add($SourceSpellingImage)
    }
    Copy-Item $SpellingImageCache $SourceSpellingImage -Force

    $ReadOnlyImageCache = & $CaptureScript -ReadOnlySave | Select-Object -Last 1
    if (-not $ReadOnlyImageCache -or -not (Test-Path $ReadOnlyImageCache)) {
        throw 'Read-only save screenshot helper did not produce an image.'
    }
    $SourceReadOnlyImage = Join-Path $SourceAssetsDir 'read-only-save.png'
    if (Test-Path $SourceReadOnlyImage) {
        $SourceAssetBackups[$SourceReadOnlyImage] = [IO.File]::ReadAllBytes($SourceReadOnlyImage)
    } else {
        $StagedFiles.Add($SourceReadOnlyImage)
    }
    Copy-Item $ReadOnlyImageCache $SourceReadOnlyImage -Force

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
        $Destination = Join-Path $DocsDir "theme\$File"
        if (Test-Path $Destination -PathType Leaf) {
            continue
        }
        $InstalledFile = Get-ChildItem -Path $MermaidStage -Filter $File -File -Recurse | Select-Object -First 1
        if (-not $InstalledFile) {
            throw "mdbook-mermaid did not install $File."
        }
        if (Test-Path $Destination) {
            throw "Refusing to overwrite existing file: $Destination"
        }
        $StagedFiles.Add($Destination)
        Copy-Item $InstalledFile.FullName $Destination -ErrorAction Stop
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
            (Join-Path $BookDir 'syntax-highlighting.html'),
            (Join-Path $BookDir 'assets\editor-screenshot.png'),
            (Join-Path $BookDir 'assets\search-panel.png'),
            (Join-Path $BookDir 'assets\syntax-highlighted-code.png'),
            (Join-Path $BookDir 'assets\syntax-language-picker.png'),
            (Join-Path $BookDir 'spelling.html'),
            (Join-Path $BookDir 'assets\spelling.png'),
            (Join-Path $BookDir 'files-and-saving.html'),
            (Join-Path $BookDir 'assets\read-only-save.png')
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
    foreach ($File in $StagedFiles) {
        Remove-Item $File -Force -ErrorAction SilentlyContinue
    }
    foreach ($Backup in $SourceAssetBackups.GetEnumerator()) {
        [IO.File]::WriteAllBytes($Backup.Key, [byte[]]$Backup.Value)
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
