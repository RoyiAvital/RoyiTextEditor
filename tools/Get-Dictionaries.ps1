<#
.SYNOPSIS
Downloads the bundled US English Hunspell dictionary into assets/dictionaries.

.DESCRIPTION
The SCOWL-based en_US Hunspell dictionary (size 60) is pinned to its release
and verified against SHA-256 hashes of the archive and of every extracted
file. The archive is cached in target/dictionary-cache (ignored by Git).
assets/dictionaries gets en_US.aff, en_US.dic and README_en_US.txt (the
copyright and license notice). The built-in spelling word list is expanded
from these files at build time; the package ships them in Dictionaries/.

.PARAMETER Force
Re-download the archive and overwrite the extracted files.

.EXAMPLE
.\tools\Get-Dictionaries.ps1
#>
[CmdletBinding()]
param(
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Root = Split-Path -Parent $PSScriptRoot
$Cache = Join-Path $Root 'target\dictionary-cache'
$Target = Join-Path $Root 'assets\dictionaries'

$Archive = @{
    Url    = 'https://downloads.sourceforge.net/project/wordlist/speller/2020.12.07/hunspell-en_US-2020.12.07.zip'
    Sha256 = '616348AD645A716D91C8A6645065E710F15E9DDA3FFEF60CDF7EC8A4E27975AF'
}
$Entries = [ordered]@{
    'en_US.aff'        = 'C7A8C4D08C29D237880844B1623099F59092602F189BE38CE3912E457FF38BC1'
    'en_US.dic'        = 'F0B1A234BD178BDD01875B2A392A9647F888B8FE879F79C52AAE62C2759B3647'
    'README_en_US.txt' = '168B4C01CB841F766A72F310B065C04D09CFF46376C37A81D799593CE751C371'
}

function Test-Hash([string] $Path, [string] $Sha256) {
    (Test-Path $Path) -and (Get-FileHash $Path -Algorithm SHA256).Hash -eq $Sha256
}

$missing = @($Entries.Keys | Where-Object { $Force -or -not (Test-Hash (Join-Path $Target $_) $Entries[$_]) })
if ($missing.Count -eq 0) {
    return
}

$archivePath = Join-Path $Cache ([IO.Path]::GetFileName(([Uri] $Archive.Url).AbsolutePath))
if ($Force -or -not (Test-Hash $archivePath $Archive.Sha256)) {
    New-Item -ItemType Directory -Force $Cache | Out-Null
    $partial = "$archivePath.partial"
    Write-Host "Downloading $($Archive.Url)"
    # SourceForge redirects browsers to a page; a plain client gets the file.
    Invoke-WebRequest -Uri $Archive.Url -OutFile $partial -UseBasicParsing -UserAgent 'Wget'
    $actual = (Get-FileHash $partial -Algorithm SHA256).Hash
    if ($actual -ne $Archive.Sha256) {
        Remove-Item $partial
        throw "SHA-256 mismatch for $($Archive.Url)`n  expected $($Archive.Sha256)`n  actual   $actual"
    }
    Move-Item -Force $partial $archivePath
}

New-Item -ItemType Directory -Force $Target | Out-Null
$zip = [IO.Compression.ZipFile]::OpenRead($archivePath)
try {
    foreach ($name in $missing) {
        $entry = $zip.Entries | Where-Object { $_.FullName -ceq $name } | Select-Object -First 1
        if (-not $entry) {
            throw "Entry '$name' not found in $archivePath"
        }
        $out = Join-Path $Target $name
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $out, $true)
        if (-not (Test-Hash $out $Entries[$name])) {
            Remove-Item $out
            throw "SHA-256 mismatch for $name in $archivePath"
        }
        Write-Host "Extracted $out"
    }
}
finally {
    $zip.Dispose()
}
