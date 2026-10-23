# Reads the application name and version from Meta/AppMeta.json, verifies them
# against Cargo.toml (and the tag in a tag build) and writes the release
# outputs. Used by .github/workflows/release.yml; runnable locally:
#
#     .\.github\scripts\Resolve-Version.ps1 [-Tag v0.1.0]
param(
    # Release tag; defaults to the tag of a GitHub Actions tag build.
    [string]$Tag = $(if ($env:GITHUB_REF_TYPE -eq 'tag') { $env:GITHUB_REF_NAME } else { '' })
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot)

$meta = Get-Content (Join-Path $root 'Meta/AppMeta.json') -Raw | ConvertFrom-Json
$name = $meta.name
$version = $meta.version
if ($name -notmatch '^[A-Za-z0-9._-]+$') {
    throw "Name '$name' in Meta/AppMeta.json must be a plain file name (letters, digits, '.', '_', '-')."
}

# `version` of [workspace.package], which every crate inherits.
$section = ''
$cargoVersion = $null
foreach ($line in Get-Content (Join-Path $root 'Cargo.toml')) {
    if ($line -match '^\s*\[(.+)\]\s*$') {
        $section = $Matches[1]
    } elseif ($section -eq 'workspace.package' -and $line -match '^\s*version\s*=\s*"([^"]*)"') {
        $cargoVersion = $Matches[1]
    }
}
if ($null -eq $cargoVersion) {
    throw 'Cargo.toml has no version in [workspace.package].'
}
if ($cargoVersion -cne $version) {
    throw "Meta/AppMeta.json version '$version' differs from Cargo.toml [workspace.package] version '$cargoVersion'."
}

$identifier = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
$semverBody = "(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-$identifier(?:\.$identifier)*)?"
if ($version -cnotmatch "^$semverBody`$") {
    throw "Version '$version' in Meta/AppMeta.json is not valid SemVer."
}

if ($Tag) {
    if ($Tag -cne "v$version") {
        throw "Tag '$Tag' does not exactly match version 'v$version'."
    }
    if ($env:GITHUB_ACTIONS -eq 'true') {
        git fetch --force origin "refs/tags/${Tag}:refs/tags/${Tag}"
        if ($LASTEXITCODE -ne 0) {
            throw "Could not fetch tag '$Tag' from origin."
        }
    }
    if ((git cat-file -t "refs/tags/$Tag") -ne 'tag') {
        throw "Tag '$Tag' must be annotated; lightweight tags cannot publish releases."
    }
    $prerelease = $version.Contains('-')
} else {
    $Tag = "v$version"
    $prerelease = $false
}

$outputs = [ordered]@{
    'app-name'      = $name
    'version'       = $version
    'tag'           = $Tag
    'prerelease'    = $prerelease.ToString().ToLower()
    'artifact-name' = "$name-$version"
}
foreach ($key in $outputs.Keys) {
    $line = "$key=$($outputs[$key])"
    if ($env:GITHUB_OUTPUT) {
        Add-Content -Path $env:GITHUB_OUTPUT -Value $line -Encoding utf8
    }
    $line
}
