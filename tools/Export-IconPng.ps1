# Saves the icon embedded in the executable as PNG files, as Windows extracts them.
# Run from the repository root after a release build.
param(
    [string]$Exe = 'target\release\RoyiTextEditor.exe',
    [string]$OutDir = 'target\icon',
    [int[]]$Sizes = @(256, 48, 32, 16)
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class IconX {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern uint PrivateExtractIcons(string file, int index, int cx, int cy, IntPtr[] icons, uint[] ids, uint count, uint flags);
    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr icon);
}
'@
$exe = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null
foreach ($s in $Sizes) {
    $handles = New-Object IntPtr[] 1
    $ids = New-Object uint32[] 1
    if ([IconX]::PrivateExtractIcons($exe, 0, $s, $s, $handles, $ids, 1, 0) -ne 1) { throw "no icon at $s" }
    $bmp = [System.Drawing.Icon]::FromHandle($handles[0]).ToBitmap()
    $bmp.Save((Join-Path (Resolve-Path $OutDir) "Icon-$s.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    [void][IconX]::DestroyIcon($handles[0])
    "Icon-$s.png $($bmp.Width)x$($bmp.Height)"
}
