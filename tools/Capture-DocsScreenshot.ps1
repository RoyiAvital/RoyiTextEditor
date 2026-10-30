[CmdletBinding()]
param(
    [switch] $SearchPanel,
    [switch] $SyntaxCode,
    [switch] $SyntaxLanguages,
    [switch] $Spelling,
    [switch] $ReadOnlySave
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$Root = Split-Path -Parent $PSScriptRoot
$EditorTarget = Join-Path $Root 'target\docs-app'
$EditorExe = Join-Path $EditorTarget 'release\RoyiTextEditor.exe'
$OutputDir = Join-Path $Root 'target\docs-assets'
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$Modes = @()
if ($SearchPanel) { $Modes += 'search' }
if ($SyntaxCode) { $Modes += 'syntax-code' }
if ($SyntaxLanguages) { $Modes += 'syntax-languages' }
if ($Spelling) { $Modes += 'spelling' }
if ($ReadOnlySave) { $Modes += 'read-only-save' }
if ($Modes.Count -gt 1) { throw 'Choose only one screenshot mode.' }

$IsSyntaxCapture = $SyntaxCode -or $SyntaxLanguages
$Fixture = if ($Spelling) {
    $SpellingFixture = Join-Path $OutputDir 'Spelling.txt'
    $SpellingSample = @'
Recieve the package before the meeting starts.
Spelling marks show misspelled words with a blue wave, like teh one here.
Press Ctrl+. on a marked word to see suggestions first, then the commands.
F8 moves to the next misspelling and Shift+F8 to the previous one.
'@
    [IO.File]::WriteAllText($SpellingFixture, $SpellingSample, (New-Object Text.UTF8Encoding $false))
    $SpellingFixture
} elseif ($ReadOnlySave) {
    # The fixture gets the Read-only attribute; the editor refuses to save it.
    $ReadOnlyFixture = Join-Path $OutputDir 'ReadOnly.txt'
    if (Test-Path $ReadOnlyFixture) {
        (Get-Item $ReadOnlyFixture).IsReadOnly = $false
    }
    $ReadOnlySample = @'
This file has the Windows Read-only attribute.
An edit is made and Ctrl+S is pressed: nothing is written.
The unsaved dot turns orange and the status line explains why.
'@
    [IO.File]::WriteAllText($ReadOnlyFixture, $ReadOnlySample, (New-Object Text.UTF8Encoding $false))
    (Get-Item $ReadOnlyFixture).IsReadOnly = $true
    $ReadOnlyFixture
} elseif ($IsSyntaxCapture) {
    $SyntaxFixture = Join-Path $OutputDir 'syntax-example.rs'
    $SyntaxSample = @'
use std::collections::HashMap;

#[derive(Debug)]
struct SyntaxOptions {
    language: &'static str,
    max_file_mib: usize,
}

fn main() {
    let mut options = HashMap::new();
    options.insert("syntax", SyntaxOptions {
        language: "rust",
        max_file_mib: 32,
    });
    println!("Highlighting: {options:?}");
}
'@
    [IO.File]::WriteAllText($SyntaxFixture, $SyntaxSample, (New-Object Text.UTF8Encoding $false))
    $SyntaxFixture
} else {
    # The editor screenshot shows these two lines (generated, like the syntax sample).
    $TextFixture = Join-Path $OutputDir 'Notes.txt'
    $TextSample = "Text is the optimal universal IO interface`nText is the final frontier of interfaces`n"
    [IO.File]::WriteAllText($TextFixture, $TextSample, (New-Object Text.UTF8Encoding $false))
    $TextFixture
}
$OutputName = if ($SearchPanel) {
    'search-panel.png'
} elseif ($SyntaxCode) {
    'syntax-highlighted-code.png'
} elseif ($SyntaxLanguages) {
    'syntax-language-picker.png'
} elseif ($Spelling) {
    'spelling.png'
} elseif ($ReadOnlySave) {
    'read-only-save.png'
} else {
    'editor-screenshot.png'
}
$Output = Join-Path $OutputDir $OutputName

& cargo build --release --locked -p rte_app --target-dir $EditorTarget
if ($LASTEXITCODE -ne 0) {
    throw "Editor build failed with exit code $LASTEXITCODE."
}

if (-not ('DocsBuildCaptureV2' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DocsBuildCaptureV2 {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool attachState);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd, ref Point point);
    [DllImport("dwmapi.dll")] public static extern int DwmFlush();
}
'@
}

[void][DocsBuildCaptureV2]::SetProcessDPIAware()
if (Test-Path $Output) {
    Remove-Item $Output -Force
}

$Arguments = @(('"{0}"' -f $Fixture))
if ($Spelling) {
    $Arguments = @('--spelling', 'builtin') + $Arguments
}
$EditorProcess = Start-Process -FilePath $EditorExe -ArgumentList $Arguments -PassThru
try {
    if (-not $EditorProcess.WaitForInputIdle(10000)) {
        throw "The editor did not become idle while opening $Fixture."
    }
    $EditorProcess.Refresh()
    $Window = $EditorProcess.MainWindowHandle
    if ($Window -eq [IntPtr]::Zero) {
        throw 'The editor window handle was not available for the documentation screenshot.'
    }
    [void][DocsBuildCaptureV2]::ShowWindow($Window, 9)
    [void][DocsBuildCaptureV2]::SetWindowPos($Window, [IntPtr](-1), 0, 0, 0, 0, 0x0043)
    $ForegroundWindow = [DocsBuildCaptureV2]::GetForegroundWindow()
    [uint32]$IgnoredProcessId = 0
    $ForegroundThread = [DocsBuildCaptureV2]::GetWindowThreadProcessId($ForegroundWindow, [ref]$IgnoredProcessId)
    $TargetThread = [DocsBuildCaptureV2]::GetWindowThreadProcessId($Window, [ref]$IgnoredProcessId)
    $CurrentThread = [DocsBuildCaptureV2]::GetCurrentThreadId()
    $AttachedForeground = $false
    $AttachedTarget = $false
    try {
        if ($ForegroundThread -ne 0 -and $ForegroundThread -ne $CurrentThread) {
            $AttachedForeground = [DocsBuildCaptureV2]::AttachThreadInput($CurrentThread, $ForegroundThread, $true)
        }
        if ($TargetThread -ne 0 -and $TargetThread -ne $CurrentThread) {
            $AttachedTarget = [DocsBuildCaptureV2]::AttachThreadInput($CurrentThread, $TargetThread, $true)
        }
        [void][DocsBuildCaptureV2]::BringWindowToTop($Window)
        [void][DocsBuildCaptureV2]::SetForegroundWindow($Window)
    }
    finally {
        if ($AttachedTarget) {
            [void][DocsBuildCaptureV2]::AttachThreadInput($CurrentThread, $TargetThread, $false)
        }
        if ($AttachedForeground) {
            [void][DocsBuildCaptureV2]::AttachThreadInput($CurrentThread, $ForegroundThread, $false)
        }
    }
    if ([DocsBuildCaptureV2]::GetForegroundWindow() -ne $Window) {
        throw 'Windows did not bring the editor window to the foreground; refusing an unrelated screenshot.'
    }
    if ($SearchPanel) {
        [System.Windows.Forms.SendKeys]::SendWait('^f')
    }
    if ($SyntaxLanguages) {
        [System.Windows.Forms.SendKeys]::SendWait('^k')
        [System.Windows.Forms.SendKeys]::SendWait('m')
        [System.Windows.Forms.SendKeys]::SendWait('^a')
        [System.Windows.Forms.SendKeys]::SendWait('{BACKSPACE}')
    }
    if ($Spelling) {
        # The marks come from the spelling thread after the first frame; the caret stays
        # on the first word (left of the palette), so its wave and the palette show together.
        Start-Sleep -Milliseconds 800
        [System.Windows.Forms.SendKeys]::SendWait('^{HOME}')
        Start-Sleep -Milliseconds 300
        [System.Windows.Forms.SendKeys]::SendWait('^.')
        Start-Sleep -Milliseconds 800
    }
    if ($ReadOnlySave) {
        [System.Windows.Forms.SendKeys]::SendWait('^{END}')
        [System.Windows.Forms.SendKeys]::SendWait('{ENTER}A new line, not saved.')
        [System.Windows.Forms.SendKeys]::SendWait('^s')
        Start-Sleep -Milliseconds 500
    }
    [void][DocsBuildCaptureV2]::DwmFlush()

    $Client = New-Object 'DocsBuildCaptureV2+Rect'
    $Origin = New-Object 'DocsBuildCaptureV2+Point'
    if (-not [DocsBuildCaptureV2]::GetClientRect($Window, [ref]$Client) -or
        -not [DocsBuildCaptureV2]::ClientToScreen($Window, [ref]$Origin)) {
        throw 'Could not determine the editor client area for the documentation screenshot.'
    }
    $Width = $Client.Right - $Client.Left
    $Height = $Client.Bottom - $Client.Top
    if ($Width -lt 800 -or $Height -lt 500) {
        throw "Editor client area is unexpectedly small: ${Width}x${Height}."
    }

    $Bitmap = New-Object System.Drawing.Bitmap -ArgumentList $Width, $Height
    try {
        $Graphics = [System.Drawing.Graphics]::FromImage($Bitmap)
        try {
            $Graphics.CopyFromScreen($Origin.X, $Origin.Y, 0, 0, $Bitmap.Size)
            $Bitmap.Save($Output, [System.Drawing.Imaging.ImageFormat]::Png)
        }
        finally {
            $Graphics.Dispose()
        }
    }
    finally {
        $Bitmap.Dispose()
    }
}
finally {
    if (-not $EditorProcess.HasExited) {
        # The refused save leaves unsaved changes: closing would only ask.
        if (-not $ReadOnlySave) {
            [void]$EditorProcess.CloseMainWindow()
        }
        if ($ReadOnlySave -or -not $EditorProcess.WaitForExit(5000)) {
            $EditorProcess.Kill()
            $EditorProcess.WaitForExit()
        }
    }
    $EditorProcess.Dispose()
    if ($ReadOnlySave) {
        (Get-Item $Fixture).IsReadOnly = $false
    }
}

Write-Output $Output