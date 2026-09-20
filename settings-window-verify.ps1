param([int]$ViewerProcessId)
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CaptionCheck {
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
$captionProcess=Get-Process -Id $ViewerProcessId
$captionWindow=[CaptionCheck]::FindWindowW([System.Management.Automation.Language.NullString]::Value, "Virtual Display Workspace")
if ($captionWindow -eq [IntPtr]::Zero) { throw 'Viewer not visible' }
$captionDpiContext=[CaptionCheck]::SetThreadDpiAwarenessContext([IntPtr](-4))
$captionSavedPointer=New-Object CaptionCheck+POINT
[void][CaptionCheck]::GetCursorPos([ref]$captionSavedPointer)
function Focus-CaptionViewer {
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
    [CaptionCheck]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
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
function Get-SettingsWindow {
    [CaptionCheck]::FindWindowW([System.Management.Automation.Language.NullString]::Value,'設定 - Virtual Display Workspace')
}
function Open-Settings {
    Focus-CaptionViewer
    Click-CaptionButton 0
    Start-Sleep -Milliseconds 600
    $h=Get-SettingsWindow
    if ($h -eq [IntPtr]::Zero) { throw 'Settings window missing' }
    return $h
}
function Set-Resolution([IntPtr]$h, [string]$label) {
    $combo=[WindowSettingsCheck]::GetDlgItem($h,10)
    $index=[WindowSettingsCheck]::SendText($combo,0x158,[IntPtr](-1),$label).ToInt32()
    if ($index -lt 0) { throw "Resolution missing: $label" }
    [void][CaptionCheck]::SendMessageW($combo,0x14E,[IntPtr]$index,[IntPtr]::Zero)
    $button=[WindowSettingsCheck]::GetDlgItem($h,1)
    if (-not [WindowSettingsCheck]::IsWindowEnabled($button)) { throw 'Apply disabled' }
    [void][CaptionCheck]::SendMessageW($button,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Seconds 3
    $current=[CaptionCheck]::SendMessageW($combo,0x147,[IntPtr]::Zero,[IntPtr]::Zero).ToInt32()
    if ($current -ne $index) { throw 'Resolution selection did not persist after apply' }
}
try {
    $h=Open-Settings
    $again=Open-Settings
    if ($h -ne $again) { throw 'Opening settings created duplicate window' }
    Start-Process -FilePath "$env:WINDIR\System32\DisplaySwitch.exe" -ArgumentList '/extend' -WindowStyle Hidden -Wait
    Start-Sleep -Seconds 2
    [void][CaptionCheck]::SendMessageW([WindowSettingsCheck]::GetDlgItem($h,5),0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Seconds 1
    Set-Resolution $h '1920 × 1200'
    $rect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($h,[ref]$rect)
    $bitmap=New-Object System.Drawing.Bitmap(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top))
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size)
        $bitmap.Save((Join-Path $PWD 'settings-window.png'))
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
    Set-Resolution $h '1920 × 1080'
    [void][CaptionCheck]::SendMessageW([WindowSettingsCheck]::GetDlgItem($h,2),0xF5,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    if ([WindowSettingsCheck]::IsWindow($h)) { throw 'Close settings failed' }
    if ($captionProcess.HasExited) { throw 'Closing settings exited viewer' }
    $h=Open-Settings
    [WindowSettingsCheck]::keybd_event(0x1B,0,0,[UIntPtr]::Zero)
    [WindowSettingsCheck]::keybd_event(0x1B,0,2,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    if ([WindowSettingsCheck]::IsWindow($h)) { throw 'Escape did not close settings' }
    $h=Open-Settings
    [void][CaptionCheck]::SendMessageW($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    if ([WindowSettingsCheck]::IsWindow($h)) { throw 'Title-bar close failed' }
    'Settings window reuse, resolution apply/restore, close button, reopen, Esc and title-bar close: PASS'
} finally {
    Start-Process -FilePath "$env:WINDIR\System32\DisplaySwitch.exe" -ArgumentList '/clone' -WindowStyle Hidden -Wait
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    [void][CaptionCheck]::SetCursorPos($captionSavedPointer.X,$captionSavedPointer.Y)
    [void][CaptionCheck]::SetThreadDpiAwarenessContext($captionDpiContext)
}

