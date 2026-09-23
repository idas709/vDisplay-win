# Virtual Display Workspace

Windows viewer for a Parsec virtual display. The Rust app creates one virtual
display for its lifetime, keeps it alive with the Parsec VDD heartbeat, captures
the resulting DXGI output, and removes the display when the app exits.

## Current implementation

- Uses `parsec-vdd-rust` to add/remove one virtual display at runtime.
- Enumerates DXGI adapters and outputs after the virtual display is added.
- Tracks the owned Parsec monitor by its PnP instance ID, refreshes its GDI name/HMONITOR, and captures the matching DXGI output with a D3D11 staging texture.
- Uploads BGRA frames to a `wgpu` texture and renders with a WGSL shader.
- Composites DXGI's separate mouse pointer into the captured frame, including color, monochrome AND/XOR and masked-color cursors. Pointer movement and visibility updates are handled even when the desktop image is unchanged.
- Maintains the logical display resolution while fitting the image to the borderless window.
- Opens at the capture display's aspect ratio, fitted within 80% of the host screen. Drag any edge or corner to resize while retaining that ratio; edge hit areas scale with Windows DPI. A changed capture aspect ratio updates the normal window's sizing constraint.
- Supports `Ctrl` + wheel zoom, middle-button or `Space` + left-button panning, `Ctrl` + `0` reset, and `Ctrl` + `N` to open an additional viewer window mirroring the virtual display.
- Hover near the top to reveal Windows-style controls flush with the upper-right corner: an "Add Window (+)" button to open an additional viewer window mirroring the virtual display, settings, fullscreen, minimize, maximize/restore and close buttons. They fade away after leaving the area. Each viewer window operates independently with its own zoom, pan, and fullscreen state (ideal for placing one window fullscreen on an extended physical monitor while keeping another on your primary monitor). Closing an individual window removes it without affecting others; closing the last remaining viewer window exits the application. The gear opens a separate modeless settings window with a virtual resolution selector, Refresh, view-mode radio buttons, and OK/Cancel. Changes are staged until OK; Cancel, Esc and the title-bar close discard them. Opening it again focuses the existing window; closing settings leaves the viewer running. Native settings controls and layout live in `src/settings.rs`, separate from application-side settings actions. Resolution changes recreate capture and update the window aspect ratio. Independent resolution changes require an extended virtual display; cloned physical outputs are not modified. F11 toggles borderless fullscreen and Esc exits it. The outer resize band stays available in windowed mode. Caption controls are rendered by wgpu using `ui/caption.wgsl`.

## Build

Install the stable Rust toolchain with the MSVC target and the Windows SDK, then run:

```text
cargo run --release
```

The Parsec VDD driver must be installed once by the installer. The virtual
display exists only while the viewer is running, so it is expected that Windows
Display Settings cannot offer an extended display before the viewer starts. On
startup the viewer adds the display and places it to the right of the primary
desktop. Desktop Duplication can return `DXGI_ERROR_ACCESS_LOST` when the output
changes; the viewer releases the old duplication/device and retries enumeration
and capture creation every 500 ms. A five-second continuous timeout also triggers
re-enumeration and recreation. A static desktop can legitimately time out, so this
watchdog is a recovery heuristic, not proof of a driver fault.

The CPU staging path is intentional for this first slice. It is straightforward
and correct, but a production 60 fps build should replace it with a shared D3D11
resource or a keyed-mutex interop path to avoid the readback/upload round trip.
Build the installer with Inno Setup using `installer/VirtualDisplayWorkspace.iss`
after placing the signed Parsec package under `driver/parsec-vdd`.


## Capture diagnostics

The installed application always appends diagnostics to
`%LOCALAPPDATA%\VirtualDisplayWorkspace\virtual-display-workspace.log`. The log
is available even though the release executable has no console window. When
started from PowerShell with standard-error redirection, the same diagnostics
are also written to the redirected output. The
settings window's Refresh button also retries the VDD connection when startup
could not connect to the driver.

```powershell
$env:RUST_LOG="info,virtual_display_workspace=debug,wgpu_core=warn,wgpu_hal=warn"
cargo run *> display-debug.log
```

Change Windows display mode while the app is running. Then inspect:

```powershell
Select-String display-debug.log -Pattern "DXGI|Acquire|duplication|Parsec|recovery|timeout"
```

Logs include hexadecimal HRESULTs, adapter LUID/index, output index/name,
desktop coordinates, attached state, HMONITOR, Parsec PnP identity and heartbeat.
A disconnected monitor has no independent desktop to capture; recovery waits
for that owned monitor to become available and does not select an unrelated
physical monitor. In Windows clone mode, the Parsec monitor can share the
primary output; capturing that shared desktop is expected.

The local dependency patch under `vendor/parsec-vdd-rust` fixes Windows monitor
identity parsing; see its PATCHES.md. Keep this directory when building.

See [DEBUGGING.md](DEBUGGING.md) for the measured display-mode recovery results.

### Floating zoom controls

Hover over the bottom-right corner to show the current Fit-relative percentage, a 100–400% slider, and a preset menu. Presets include 100/150/200/300/400%, actual pixel size, and actual size ×1.5/×2/×3/×4. Actual-size presets are disabled if `multiple / fit_scale` is outside 1–4, and availability follows viewport/content resizing. Dragging preserves the viewer's center. The panel fades after leaving; an open menu remains visible until selection, an outside click, Esc or focus loss. Short windows use a scrollable menu. Runtime implementation: `src/zoom_ui.rs` and `ui/zoom.wgsl`.

The UI uses Windows system fonts: Yu Gothic UI for Japanese and Segoe UI for Latin text, with explicit script runs for mixed zoom labels. No font downloads, installation or bundled font files are required. The slider thumb uses analytic antialiasing at physical pixel resolution.

Settings includes a Japanese / English language selector. OK applies the language to settings and zoom menus; Cancel discards the selection. The preference is saved in `%LOCALAPPDATA%\VirtualDisplayWorkspace\language.txt` for the next launch. Japanese is the default. Translation selection and display-error translations live in `src/language.rs`.


Build the installer with `.\installer\build.ps1`; see [installer instructions](installer/README.md).
