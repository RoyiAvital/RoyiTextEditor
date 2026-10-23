# Extracts the CHANGELOG.md section of one version as release notes and checks
# release readiness. Used by .github/workflows/release.yml; runnable locally:
#
#     .\.github\scripts\Get-ReleaseNotes.ps1 -Version 0.2.0 [-Output notes.md] [-DryRun]
#
# -DryRun takes the "## [Unreleased]" section instead of requiring "## [X.Y.Z]".
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$Output,
    [switch]$DryRun,
    [string]$Changelog = (Join-Path (Split-Path (Split-Path $PSScriptRoot)) 'CHANGELOG.md')
)
$ErrorActionPreference = 'Stop'

$text = [IO.File]::ReadAllText($Changelog).Replace("`r`n", "`n")

# Body after `$heading` up to the next "## " heading, trimmed.
function Get-SectionBody([System.Text.RegularExpressions.Match]$heading) {
    $start = $heading.Index + $heading.Length
    $next = [regex]::Match($text.Substring($start), '(?m)^## ')
    $end = if ($next.Success) { $start + $next.Index } else { $text.Length }
    $text.Substring($start, $end - $start).Trim()
}

$unreleased = [regex]::Match($text, '(?m)^## \[Unreleased\][ \t]*$')
if (-not $unreleased.Success) {
    throw 'CHANGELOG.md has no "## [Unreleased]" section.'
}
if ([regex]::IsMatch($text.Substring(0, $unreleased.Index), '(?m)^## ')) {
    throw '"## [Unreleased]" must be the first changelog section.'
}

if ($DryRun) {
    $notes = "Dry run for $Version.`n`n$(Get-SectionBody $unreleased)`n"
} else {
    $pattern = '(?m)^## \[' + [regex]::Escape($Version) + '\](?: - (\d{4}-\d{2}-\d{2}))?[ \t]*$'
    $heading = [regex]::Match($text, $pattern)
    if (-not $heading.Success) {
        throw "CHANGELOG.md has no `"## [$Version]`" section. Move the `"## [Unreleased]`" entries under `"## [$Version]`" before tagging."
    }
    $section = Get-SectionBody $heading
    $date = $heading.Groups[1]
    $notes = if ($date.Success) { "Released $($date.Value).`n`n$section`n" } else { "$section`n" }
}

if ($Output) {
    $path = [IO.Path]::GetFullPath([IO.Path]::Combine((Get-Location).Path, $Output))
    New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
    [IO.File]::WriteAllText($path, $notes, (New-Object Text.UTF8Encoding $false))
} else {
    $notes
}
