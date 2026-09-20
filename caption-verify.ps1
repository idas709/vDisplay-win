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
$captionResults=[ordered]@{}
try {
    Focus-CaptionViewer
    Move-CaptionPointer -1
    Start-Sleep -Milliseconds 600
    Save-CaptionScreenshot 'caption-hidden.png'
    Move-CaptionPointer 1
    Save-CaptionScreenshot 'caption-visible.png'
    Click-CaptionButton 0
    $captionMenu=[CaptionCheck]::FindWindowW([System.Management.Automation.Language.NullString]::Value,'設定 - Virtual Display Workspace')
    $captionResults.SettingsWindow=($captionMenu -ne [IntPtr]::Zero)
    if (-not $captionResults.SettingsWindow) { throw 'Settings window did not open' }
    Save-CaptionScreenshot 'caption-settings.png'
    [void][CaptionCheck]::SendMessageW($captionMenu,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    Start-Sleep -Milliseconds 150
    Click-CaptionButton 2
    $captionResults.Minimize=[CaptionCheck]::IsIconic($captionWindow)
    if (-not $captionResults.Minimize) { throw 'Minimize button failed' }
    [void][CaptionCheck]::ShowWindow($captionWindow,9)
    Focus-CaptionViewer
    Click-CaptionButton 3
    $captionResults.Maximize=[CaptionCheck]::IsZoomed($captionWindow)
    if (-not $captionResults.Maximize) { throw 'Maximize button failed' }
    Move-CaptionPointer 3
    Save-CaptionScreenshot 'caption-maximized.png'
    Click-CaptionButton 3
    $captionResults.Restore=-not [CaptionCheck]::IsZoomed($captionWindow)
    if (-not $captionResults.Restore) { throw 'Restore button failed' }
    Move-CaptionPointer 4
    Save-CaptionScreenshot 'caption-close-hover.png'
    [CaptionCheck]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    Move-CaptionPointer -1
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 150
    $captionResults.CancelClose=-not $captionProcess.HasExited
    if (-not $captionResults.CancelClose) { throw 'Dragging off close incorrectly closed viewer' }
    $captionRect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($captionWindow,[ref]$captionRect)
    $captionX=$captionRect.Right-2
    $captionY=$captionRect.Top+2
    $captionPacked=([long]($captionY -band 65535) -shl 16) -bor ($captionX -band 65535)
    $captionResults.ResizeHit=[CaptionCheck]::SendMessageW($captionWindow,0x84,[IntPtr]::Zero,[IntPtr]$captionPacked).ToInt32()
    if ($captionResults.ResizeHit -ne 14) { throw 'Top-right resize band no longer works' }
    Click-CaptionButton 4
    $captionResults.Close=$captionProcess.WaitForExit(4000)
    if (-not $captionResults.Close) { throw 'Close button failed' }
} finally {
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    [void][CaptionCheck]::SetCursorPos($captionSavedPointer.X,$captionSavedPointer.Y)
    [void][CaptionCheck]::SetThreadDpiAwarenessContext($captionDpiContext)
}
$captionResults | ConvertTo-Json | Set-Content caption-verification.json
$captionResults
