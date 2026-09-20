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
    Start-Sleep -Milliseconds 500
}
function Click-CaptionButton([int]$Index) {
    Move-CaptionPointer $Index
    $captionPoint=New-Object CaptionCheck+POINT
    [void][CaptionCheck]::GetCursorPos([ref]$captionPoint)
    if ([CaptionCheck]::WindowFromPoint($captionPoint) -ne $captionWindow) { throw 'Another window covers the button; aborting click' }
    [CaptionCheck]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 800
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
public static class SettingsCheck { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuStringW(IntPtr menu, uint item, StringBuilder text, int count, uint flags);
    [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr menu, uint item, uint flags);
    [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr h, IntPtr menu, uint item, out RECT rect);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
}
'@
function Read-SettingsMenu {
    Click-CaptionButton 0
    $popup=[CaptionCheck]::FindWindowW('#32768',[System.Management.Automation.Language.NullString]::Value)
    if ($popup -eq [IntPtr]::Zero) { throw 'Settings popup missing' }
    $menu=[CaptionCheck]::SendMessageW($popup,0x1E1,[IntPtr]::Zero,[IntPtr]::Zero)
    $items=@()
    for ($i=0; $i -lt [SettingsCheck]::GetMenuItemCount($menu); $i++) {
        $label=New-Object Text.StringBuilder 512
        [void][SettingsCheck]::GetMenuStringW($menu,$i,$label,512,0x400)
        $rect=New-Object SettingsCheck+RECT
        [void][SettingsCheck]::GetMenuItemRect($captionWindow,$menu,$i,[ref]$rect)
        $items += [pscustomobject]@{ Label=$label.ToString(); Checked=([SettingsCheck]::GetMenuState($menu,$i,0x400) -band 8) -ne 0; Rect=$rect }
    }
    return $items
}
function Choose-Resolution($item) {
    if ($null -eq $item) { throw 'Requested resolution not in menu' }
    [void][CaptionCheck]::SetCursorPos([int](($item.Rect.Left+$item.Rect.Right)/2),[int](($item.Rect.Top+$item.Rect.Bottom)/2))
    [CaptionCheck]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Seconds 3
}
try {
    Focus-CaptionViewer
    $before=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($captionWindow,[ref]$before)
    Click-CaptionButton 1
    $full=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($captionWindow,[ref]$full)
    Save-CaptionScreenshot 'settings-fullscreen.png'
    $packed=([long](($full.Top+1) -band 65535) -shl 16) -bor (($full.Right-1) -band 65535)
    $hit=[CaptionCheck]::SendMessageW($captionWindow,0x84,[IntPtr]::Zero,[IntPtr]$packed).ToInt32()
    if ($hit -ne 1) { throw 'Fullscreen edge must be client area' }
    Click-CaptionButton 1
    $restored=New-Object CaptionCheck+RECT
    [void][CaptionCheck]::GetWindowRect($captionWindow,[ref]$restored)
    if ($restored.Right-$restored.Left -ne $before.Right-$before.Left -or $restored.Bottom-$restored.Top -ne $before.Bottom-$before.Top) { throw 'Fullscreen did not restore size' }
    $items=@(Read-SettingsMenu)
    $items | Select-Object Label,Checked | ConvertTo-Json | Set-Content settings-menu-items.json
    $original=$items | Where-Object Checked | Select-Object -First 1
    $choice=$items | Where-Object { $_.Label -eq '1920 × 1200' } | Select-Object -First 1
    Choose-Resolution $choice
    $items=@(Read-SettingsMenu)
    if (-not ($items | Where-Object { $_.Label -eq $choice.Label -and $_.Checked })) { throw 'Resolution did not change' }
    Choose-Resolution ($items | Where-Object { $_.Label -eq $original.Label } | Select-Object -First 1)
    $items=@(Read-SettingsMenu)
    if (-not ($items | Where-Object { $_.Label -eq $original.Label -and $_.Checked })) { throw 'Original resolution did not restore' }
    [void][CaptionCheck]::SendMessageW($captionWindow,0x1F,[IntPtr]::Zero,[IntPtr]::Zero)
    [pscustomobject]@{ Fullscreen=$full; Restored=$restored; FullscreenHit=$hit; Resolution=$choice.Label; Original=$original.Label } | ConvertTo-Json | Set-Content settings-verification.json
    Get-Content settings-verification.json
} finally {
    [void][CaptionCheck]::SendMessageW($captionWindow,0x1F,[IntPtr]::Zero,[IntPtr]::Zero)
    [CaptionCheck]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    [void][CaptionCheck]::SetCursorPos($captionSavedPointer.X,$captionSavedPointer.Y)
    [void][CaptionCheck]::SetThreadDpiAwarenessContext($captionDpiContext)
}
