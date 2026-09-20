param([int]$ViewerProcessId)
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CaptionCheck {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
}
'@
function Open-LanguageSettings([string]$title) {
    Focus-CaptionViewer
    Click-CaptionButton 0
    Start-Sleep -Milliseconds 600
    $h=[CaptionCheck]::FindWindowW([System.Management.Automation.Language.NullString]::Value,$title)
    if ($h -eq [IntPtr]::Zero) { throw "Settings missing: $title" }
    return $h
}
function Choose-Language($h,[int]$index,[int]$button) {
    [void][CaptionCheck]::SendMessageW([WindowSettingsCheck]::GetDlgItem($h,11),0x14E,[IntPtr]$index,[IntPtr]::Zero)
    [void][CaptionCheck]::SendMessageW([WindowSettingsCheck]::GetDlgItem($h,$button),0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 700
    if ([WindowSettingsCheck]::IsWindow($h)) { throw 'Settings did not close' }
}
function Save-LanguageWindow($h) {
    $rect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($h,[ref]$rect)
    $bitmap=New-Object System.Drawing.Bitmap(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top))
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    try {$graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size);$bitmap.Save((Join-Path $PWD 'settings-english.png'))}
    finally {$graphics.Dispose();$bitmap.Dispose()}
}
$captionProcess=Get-Process -Id $ViewerProcessId
$captionWindow=[CaptionCheck]::FindWindowW([System.Management.Automation.Language.NullString]::Value, "Virtual Display Workspace")
if ($captionWindow -eq [IntPtr]::Zero) { throw 'Viewer not visible' }
$captionDpiContext=[CaptionCheck]::SetThreadDpiAwarenessContext([IntPtr](-4))
$captionSavedPointer=New-Object CaptionCheck+POINT
[void][CaptionCheck]::GetCursorPos([ref]$captionSavedPointer)
function Focus-CaptionViewer {
    [void][CaptionCheck]::ShowWindow($captionWindow,9)
    [void][CaptionCheck]::SetWindowPos($captionWindow,[IntPtr](-1),0,0,0,0,0x13)
    [WindowSettingsCheck]::keybd_event(0x12,0,0,[UIntPtr]::Zero)
    [WindowSettingsCheck]::keybd_event(0x12,0,2,[UIntPtr]::Zero)
    $captionForegroundThread=[CaptionCheck]::GetWindowThreadProcessId([CaptionCheck]::GetForegroundWindow(),[IntPtr]::Zero)
    $captionCurrentThread=[CaptionCheck]::GetCurrentThreadId()
    $captionAttached=[CaptionCheck]::AttachThreadInput($captionCurrentThread,$captionForegroundThread,$true)
    try { [void][CaptionCheck]::SetForegroundWindow($captionWindow) }
    finally { if ($captionAttached) { [void][CaptionCheck]::AttachThreadInput($captionCurrentThread,$captionForegroundThread,$false) } }
    Start-Sleep -Milliseconds 150
    if ([CaptionCheck]::GetForegroundWindow() -ne $captionWindow) { throw 'Viewer not foreground; aborting clicks' }
}
function Move-CaptionPointer([int]$Index) {
    $captionRect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetClientRect($captionWindow,[ref]$captionRect)
    $captionScale=[CaptionCheck]::GetDpiForWindow($captionWindow)/96.0
    $captionPoint=New-Object CaptionCheck+POINT
    if ($Index -lt 0) { $captionPoint.X=[int]($captionRect.Right/2); $captionPoint.Y=[int]($captionRect.Bottom/2) }
    else { $captionPoint.X=[int]($captionRect.Right-(46*(4-$Index)+23)*$captionScale); $captionPoint.Y=[int](16*$captionScale) }
    [void][CaptionCheck]::ClientToScreen($captionWindow,[ref]$captionPoint)
    [void][CaptionCheck]::SetCursorPos($captionPoint.X,$captionPoint.Y)
    Start-Sleep -Milliseconds 250
}
function Click-CaptionButton([int]$Index) {
    Move-CaptionPointer $Index
    $captionPoint=New-Object CaptionCheck+POINT
    [void][CaptionCheck]::GetCursorPos([ref]$captionPoint)
    if ([CaptionCheck]::WindowFromPoint($captionPoint) -ne $captionWindow) { throw 'Another window covers the button; aborting click' }
    [void][CaptionCheck]::SendMessageW($captionWindow,0x201,[IntPtr]1,[IntPtr]::Zero)
    [void][CaptionCheck]::SetCursorPos($captionPoint.X,$captionPoint.Y)
    [void][CaptionCheck]::SendMessageW($captionWindow,0x202,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 300
}
function Save-CaptionScreenshot([string]$Name) {
    $captionRect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($captionWindow,[ref]$captionRect)
    $captionScale=[CaptionCheck]::GetDpiForWindow($captionWindow)/96.0
    $captionWidth=[Math]::Min(($captionRect.Right-$captionRect.Left),[int](400*$captionScale))
    $captionHeight=[int](100*$captionScale)
    $captionBitmap=New-Object System.Drawing.Bitmap($captionWidth,$captionHeight)
    $captionGraphics=[System.Drawing.Graphics]::FromImage($captionBitmap)
    try {
        $captionGraphics.CopyFromScreen(($captionRect.Right-$captionWidth),$captionRect.Top,0,0,$captionBitmap.Size)
        $captionBitmap.Save((Join-Path (Get-Location) $Name),[System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $captionGraphics.Dispose(); $captionBitmap.Dispose() }
}
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class WindowSettingsCheck {
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr SendText(IntPtr h, uint m, IntPtr w, string text);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, UIntPtr e);
}
'@
try {
    $h=Open-LanguageSettings '設定 - Virtual Display Workspace'
    Choose-Language $h 1 2
    $h=Open-LanguageSettings '設定 - Virtual Display Workspace'
    if ([CaptionCheck]::SendMessageW([WindowSettingsCheck]::GetDlgItem($h,11),0x147,[IntPtr]::Zero,[IntPtr]::Zero).ToInt32() -ne 0) { throw 'Cancel changed language' }
    Choose-Language $h 1 1
    $h=Open-LanguageSettings 'Settings - Virtual Display Workspace'
    Save-LanguageWindow $h
    if ((Get-Content (Join-Path $env:LOCALAPPDATA 'VirtualDisplayWorkspace\language.txt')) -ne 'en') { throw 'English was not saved' }
    [void][CaptionCheck]::SendMessageW($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    [void][CaptionCheck]::SendMessageW($captionWindow,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    if (-not $captionProcess.WaitForExit(4000)) { throw 'Viewer did not exit' }
    $captionProcess=Start-Process -FilePath 'target\release\virtual-display-workspace.exe' -WorkingDirectory $PWD -RedirectStandardOutput 'language-restart.log' -RedirectStandardError 'language-restart-stderr.log' -PassThru
    Start-Sleep -Seconds 4
    $captionWindow=[CaptionCheck]::FindWindowW([System.Management.Automation.Language.NullString]::Value,'Virtual Display Workspace')
    $h=Open-LanguageSettings 'Settings - Virtual Display Workspace'
    if ([CaptionCheck]::SendMessageW([WindowSettingsCheck]::GetDlgItem($h,11),0x147,[IntPtr]::Zero,[IntPtr]::Zero).ToInt32() -ne 1) { throw 'Restart lost English preference' }
    [void][CaptionCheck]::SendMessageW($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    .\zoom-ui-verify.ps1 -ViewerProcessId $captionProcess.Id
    $h=Open-LanguageSettings 'Settings - Virtual Display Workspace'
    Choose-Language $h 0 1
    $h=Open-LanguageSettings '設定 - Virtual Display Workspace'
    [void][CaptionCheck]::SendMessageW($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    [void][CaptionCheck]::SendMessageW($captionWindow,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    'PASS: Cancel preserves language; OK switches language; restart retains English; Japanese restored.'
} finally {
    [void][CaptionCheck]::SetCursorPos($captionSavedPointer.X,$captionSavedPointer.Y)
    [void][CaptionCheck]::SetWindowPos($captionWindow,[IntPtr](-2),0,0,0,0,0x13)
    [void][CaptionCheck]::SetThreadDpiAwarenessContext($captionDpiContext)
}
