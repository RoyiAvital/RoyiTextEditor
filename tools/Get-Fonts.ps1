<#
.SYNOPSIS
Downloads the bundled monospaced fonts into assets/fonts.

.DESCRIPTION
Every download is pinned to a release tag and verified against its SHA-256
hash before use. Archives are cached in target/font-cache (ignored by Git);
only the needed faces and the license are extracted. Each family folder gets
its TrueType faces and its SIL Open Font License as OFL.txt.

.PARAMETER Family
Families to fetch (default: all). Names: JetBrainsMono, CascadiaMono,
FiraMono, IosevkaFixed, Lilex.

.PARAMETER Force
Re-download cached files and overwrite existing font files.

.EXAMPLE
.\tools\Get-Fonts.ps1
.\tools\Get-Fonts.ps1 -Family Lilex, FiraMono -Force
#>
[CmdletBinding()]
param(
    [ValidateSet('JetBrainsMono', 'CascadiaMono', 'FiraMono', 'IosevkaFixed', 'Lilex')]
    [string[]] $Family,
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 downloads far slower with the progress bar.
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Root = Split-Path -Parent $PSScriptRoot
$Cache = Join-Path $Root 'target\font-cache'
$Fonts = Join-Path $Root 'assets\fonts'

# Archive entries map to output names; Files are single downloads.
$Families = @(
    @{
        Name    = 'JetBrainsMono'
        Archive = @{
            Url    = 'https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip'
            Sha256 = '6F6376C6ED2960EA8A963CD7387EC9D76E3F629125BC33D1FDCD7EB7012F7BBF'
        }
        Entries = [ordered]@{
            'fonts/ttf/JetBrainsMono-Regular.ttf'    = 'JetBrainsMono-Regular.ttf'
            'fonts/ttf/JetBrainsMono-Bold.ttf'       = 'JetBrainsMono-Bold.ttf'
            'fonts/ttf/JetBrainsMono-Italic.ttf'     = 'JetBrainsMono-Italic.ttf'
            'fonts/ttf/JetBrainsMono-BoldItalic.ttf' = 'JetBrainsMono-BoldItalic.ttf'
            'OFL.txt'                                = 'OFL.txt'
        }
        Files   = @()
    },
    @{
        Name    = 'CascadiaMono'
        Archive = @{
            Url    = 'https://github.com/microsoft/cascadia-code/releases/download/v2407.24/CascadiaCode-2407.24.zip'
            Sha256 = 'E67A68EE3386DB63F48B9054BD196EA752BC6A4EBB4DF35ADCE6733DA50C8474'
        }
        Entries = [ordered]@{
            'ttf/static/CascadiaMono-Regular.ttf'    = 'CascadiaMono-Regular.ttf'
            'ttf/static/CascadiaMono-Bold.ttf'       = 'CascadiaMono-Bold.ttf'
            'ttf/static/CascadiaMono-Italic.ttf'     = 'CascadiaMono-Italic.ttf'
            'ttf/static/CascadiaMono-BoldItalic.ttf' = 'CascadiaMono-BoldItalic.ttf'
        }
        Files   = @(
            @{
                Url    = 'https://raw.githubusercontent.com/microsoft/cascadia-code/v2407.24/LICENSE'
                Sha256 = '51882CD3CDBA4E16F220F44DDB08A635C38C44EA6E0975DB2574F4BE6F958238'
                Name   = 'OFL.txt'
            }
        )
    },
    @{
        # Fira Mono has no italic faces.
        Name    = 'FiraMono'
        Archive = $null
        Entries = [ordered]@{}
        Files   = @(
            @{
                Url    = 'https://raw.githubusercontent.com/mozilla/Fira/4.202/ttf/FiraMono-Regular.ttf'
                Sha256 = '8C86F2963208A353C3435E28ECB38A99AB68F14D7433BA00C1822CEF9A9C1B44'
                Name   = 'FiraMono-Regular.ttf'
            },
            @{
                Url    = 'https://raw.githubusercontent.com/mozilla/Fira/4.202/ttf/FiraMono-Bold.ttf'
                Sha256 = '2A28EFD740E8DA1DA75D40AC79F0DB4F60A0F1AEAD0B15CA16B4694A11B45FC6'
                Name   = 'FiraMono-Bold.ttf'
            },
            @{
                Url    = 'https://raw.githubusercontent.com/mozilla/Fira/4.202/LICENSE'
                Sha256 = '3D70884EEDC6B82C9563E1848BC2A8189AACF1743E39D8FC4AFB860C6DA031A5'
                Name   = 'OFL.txt'
            }
        )
    },
    @{
        # The Fixed variant: strictly monospaced, no ligatures.
        Name    = 'IosevkaFixed'
        Archive = @{
            Url    = 'https://github.com/be5invis/Iosevka/releases/download/v34.9.0/PkgTTF-IosevkaFixed-34.9.0.zip'
            Sha256 = 'C3204F46863AF9E7F0AE06F0BD0B3414697C424DBA8573DAF37C3053A76213B7'
        }
        Entries = [ordered]@{
            'IosevkaFixed-Regular.ttf'    = 'IosevkaFixed-Regular.ttf'
            'IosevkaFixed-Bold.ttf'       = 'IosevkaFixed-Bold.ttf'
            'IosevkaFixed-Italic.ttf'     = 'IosevkaFixed-Italic.ttf'
            'IosevkaFixed-BoldItalic.ttf' = 'IosevkaFixed-BoldItalic.ttf'
        }
        Files   = @(
            @{
                Url    = 'https://raw.githubusercontent.com/be5invis/Iosevka/v34.9.0/LICENSE.md'
                Sha256 = '4BA53C7C1CB39279AAE5F8D7D22054C485C71169920E5A36ED098B115E2E3C5D'
                Name   = 'OFL.txt'
            }
        )
    },
    @{
        Name    = 'Lilex'
        Archive = @{
            Url    = 'https://github.com/mishamyrt/Lilex/releases/download/2.630/Lilex.zip'
            Sha256 = 'C946BF022F22895799EB69BCC5F2318ED4089E4E34B90054DFD78ED5316728E0'
        }
        Entries = [ordered]@{
            'ttf/Lilex-Regular.ttf'    = 'Lilex-Regular.ttf'
            'ttf/Lilex-Bold.ttf'       = 'Lilex-Bold.ttf'
            'ttf/Lilex-Italic.ttf'     = 'Lilex-Italic.ttf'
            'ttf/Lilex-BoldItalic.ttf' = 'Lilex-BoldItalic.ttf'
        }
        Files   = @(
            @{
                Url    = 'https://raw.githubusercontent.com/mishamyrt/Lilex/2.630/OFL.txt'
                Sha256 = 'CC3BD94CF51F40137E353FC9CA0859D54EC07E59C2092270B64A0723F4B34011'
                Name   = 'OFL.txt'
            }
        )
    }
)

function Get-VerifiedDownload([string] $Url, [string] $Sha256) {
    $name = [IO.Path]::GetFileName(([Uri] $Url).AbsolutePath)
    $folder = Join-Path $Cache ([IO.Path]::GetFileName([IO.Path]::GetDirectoryName(([Uri] $Url).AbsolutePath)))
    $path = Join-Path $folder $name
    if (-not $Force -and (Test-Path $path) -and (Get-FileHash $path -Algorithm SHA256).Hash -eq $Sha256) {
        return $path
    }
    New-Item -ItemType Directory -Force $folder | Out-Null
    $partial = "$path.partial"
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $partial -UseBasicParsing
    $actual = (Get-FileHash $partial -Algorithm SHA256).Hash
    if ($actual -ne $Sha256) {
        Remove-Item $partial
        throw "SHA-256 mismatch for $Url`n  expected $Sha256`n  actual   $actual"
    }
    Move-Item -Force $partial $path
    return $path
}

$selected = if ($Family) { $Families | Where-Object { $Family -contains $_.Name } } else { $Families }
$written = New-Object System.Collections.Generic.List[object]

foreach ($f in $selected) {
    $target = Join-Path $Fonts $f.Name
    New-Item -ItemType Directory -Force $target | Out-Null

    if ($f.Archive) {
        $archive = Get-VerifiedDownload $f.Archive.Url $f.Archive.Sha256
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        try {
            foreach ($entryName in $f.Entries.Keys) {
                $out = Join-Path $target $f.Entries[$entryName]
                if (-not $Force -and (Test-Path $out)) {
                    $written.Add((Get-Item $out))
                    continue
                }
                $entry = $zip.Entries | Where-Object { $_.FullName -ceq $entryName } | Select-Object -First 1
                if (-not $entry) {
                    throw "Entry '$entryName' not found in $archive"
                }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $out, $true)
                $written.Add((Get-Item $out))
            }
        }
        finally {
            $zip.Dispose()
        }
    }

    foreach ($file in $f.Files) {
        $out = Join-Path $target $file.Name
        if (-not $Force -and (Test-Path $out) -and (Get-FileHash $out -Algorithm SHA256).Hash -eq $file.Sha256) {
            $written.Add((Get-Item $out))
            continue
        }
        Copy-Item -Force (Get-VerifiedDownload $file.Url $file.Sha256) $out
        $written.Add((Get-Item $out))
    }
}

# TrueType files start with version 0x00010000; anything else is corrupt.
foreach ($file in $written | Where-Object { $_.Extension -eq '.ttf' }) {
    $head = [IO.File]::ReadAllBytes($file.FullName)[0..3] -join ','
    if ($head -ne '0,1,0,0') {
        throw "$($file.FullName) is not a TrueType font."
    }
}

$written | ForEach-Object {
    [pscustomobject]@{
        Family = $_.Directory.Name
        File   = $_.Name
        KiB    = [math]::Round($_.Length / 1KB)
    }
} | Format-Table -AutoSize
