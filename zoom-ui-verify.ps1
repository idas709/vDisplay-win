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
function Move-Zoom([double]$X, [double]$Y) {
    $rect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetClientRect($captionWindow,[ref]$rect)
    $scale=[CaptionCheck]::GetDpiForWindow($captionWindow)/96.0
    $point=New-Object CaptionCheck+POINT
    $point.X=[int]($rect.Right+$X*$scale); $point.Y=[int]($rect.Bottom+$Y*$scale)
    [void][CaptionCheck]::ClientToScreen($captionWindow,[ref]$point)
    [void][CaptionCheck]::SetCursorPos($point.X,$point.Y)
    Start-Sleep -Milliseconds 300
}
function Click-Zoom([double]$X, [double]$Y) {
    Move-Zoom $X $Y
    [CaptionCheck]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
}
function Save-Zoom([string]$Name) {
    $rect=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetClientRect($captionWindow,[ref]$rect)
    $scale=[CaptionCheck]::GetDpiForWindow($captionWindow)/96.0
    $point=New-Object CaptionCheck+POINT
    $point.X=$rect.Right; $point.Y=$rect.Bottom
    [void][CaptionCheck]::ClientToScreen($captionWindow,[ref]$point)
    $bitmap=New-Object System.Drawing.Bitmap([int](400*$scale),[int](380*$scale))
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen(($point.X-$bitmap.Width),($point.Y-$bitmap.Height),0,0,$bitmap.Size)
        $bitmap.Save((Join-Path $PWD $Name))
    } finally { $graphics.Dispose();$bitmap.Dispose() }
}
try {
    Focus-CaptionViewer
    Move-CaptionPointer -1
    Start-Sleep -Milliseconds 800
    Save-Zoom 'zoom-hidden.png'
    Move-Zoom -100 -35
    Save-Zoom 'zoom-visible.png'
    Click-Zoom -60 -38
    Save-Zoom 'zoom-menu.png'
    # Menu row 9: actual x4, disabled when Fit scale is 0.8.
    Click-Zoom -160 -88
    Save-Zoom 'zoom-disabled-click.png'
    # Menu row 2: 200%.
    Click-Zoom -160 -284
    Save-Zoom 'zoom-200.png'
    Move-Zoom -220 -38
    [CaptionCheck]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Move-Zoom -132 -38
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
    Save-Zoom 'zoom-400.png'
    Click-Zoom -60 -38
    # Actual 1x = 125% at initial viewer size.
    Click-Zoom -160 -200
    Save-Zoom 'zoom-actual.png'
    Move-CaptionPointer -1
    Start-Sleep -Milliseconds 900
    Save-Zoom 'zoom-hidden-after.png'
    'Zoom hover, menu, disabled preset, preset selection, slider and auto-hide exercised.'
} finally {
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    [void][CaptionCheck]::SetCursorPos($captionSavedPointer.X,$captionSavedPointer.Y)
    [void][CaptionCheck]::SetThreadDpiAwarenessContext($captionDpiContext)
}
