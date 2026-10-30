# Sets the version of the workspace crates in Cargo.lock to the one in
# Meta/AppMeta.json, so a release only edits Cargo.toml and AppMeta.json.
# Dependency entries are untouched; the later --locked steps still verify them.
#
#     .\.github\scripts\Sync-LockVersions.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot)

$version = (Get-Content (Join-Path $root 'Meta/AppMeta.json') -Raw | ConvertFrom-Json).version
$path = Join-Path $root 'Cargo.lock'
$text = [IO.File]::ReadAllText($path)
# Workspace crates are the packages without a `source`.
$blocks = foreach ($block in ($text -split '(?=\[\[package\]\])')) {
    if ($block.StartsWith('[[package]]') -and $block -notmatch '(?m)^source = ') {
        $block -replace '(?m)^version = "[^"]*"', "version = `"$version`""
    } else {
        $block
    }
}
$synced = -join $blocks
if ($synced -cne $text) {
    [IO.File]::WriteAllText($path, $synced)
    "Cargo.lock: workspace crates set to $version"
}
