# Display recovery investigation — 2026-09-06 JST

## Auto-hiding caption controls — 2026-09-06 JST

The viewer renders a DPI-scaled caption overlay directly over the capture using
ui/caption.wgsl. It is flush with the top-right corner (no inset or rounded
floating panel). Left to right: Settings, Minimize, Maximize/Restore, Close.
The gear opens a native Windows popup for Fit and Actual Size. Moving near the
top reveals the controls with a short fade; leaving waits 350 ms before fading
out. The row compresses slightly for the minimum portrait window width.

Buttons activate on release over the pressed control, support cancel by dragging
off, and do not respond while hidden. The eight-DIP native resize band remains
available in normal windows; maximized controls remain clickable at the outer
corner. Hover detection also works over nonclient resize edges and stops when
another window covers the viewer. Showing controls does not change window size
or the capture viewport.

Final validation: cargo check, 15 unit tests, and cargo build --release passed.
The Release GUI test caption-verify.ps1 passed SettingsMenu, Minimize, Maximize,
Restore, CancelClose, ResizeHit (HTTOPRIGHT=14), and Close. See
caption-verification.json and caption-verified.log. The test ends by using the
Close button, and restores the pointer position. Screenshots caption-hidden.png,
caption-visible.png, caption-maximized.png, caption-close-hover.png and
caption-settings.png document the rendered states. These are Windows-style
custom controls; the settings popup is a native menu.

## Fixed-aspect window resizing — 2026-09-06 JST

The borderless viewer now installs a native window subclass. WM_NCHITTEST exposes
an eight-DIP resize band on all four edges and corners, before client-area overlay
hit tests. WM_SIZING constrains the proposed client dimensions to the capture
aspect ratio before Windows applies them. Opposite edges/corners stay anchored;
corner drags use both axes. The shorter side has a 180-DIP minimum. Nonclient
dimensions and signed monitor coordinates are accounted for.

The window is initially hidden until its capture ratio and initial size are set,
then appears centered at a size fitting within 80% of its host monitor. Recovery
updates the ratio if the capture dimensions change aspect. Standard maximization
continues to use the host screen; viewport Fit preserves the image ratio there.

Validation: cargo check, all 10 unit tests (four sizing and six cursor tests),
and cargo build --release passed. Live Release mouse drags exercised all eight
edges/corners with the expected native hit-test codes. Results are stored in
resize-verification.json (shrinking) and resize-verification-growth.json
(bottom-right enlargement). All measured client sizes remained within one pixel
of the intended 16:9 dimensions, allowing integer rounding. The startup log
resize-debug.log records capture 1920x1080 and requested window 1536x864.
Each drag test restored the original window bounds and pointer position.

API references:
https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-sizing
https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-nchittest

## Cursor composition fix — 2026-09-06 JST

The capture path previously ignored DXGI_OUTDUPL_FRAME_INFO.PointerPosition and
PointerShapeBufferSize. DXGI can supply the pointer separately from the desktop
texture, so copying only that texture omits the hardware cursor.

The capture path now caches GetFramePointerShape data, retains position/visibility
when no mouse update is reported, and composites visible separate pointers into
each freshly copied frame before viewport rendering. Color alpha, monochrome
AND/XOR and masked-color XOR are supported, with clipping at output boundaries.
DXGI positions are output-local top-left coordinates; the hotspot is not subtracted.
Duplication recreation resets pointer state. Embedded cursors are not drawn twice.

Validation: cargo check, six pointer composition tests and cargo build --release
passed. cursor-debug.log at 16:45:10 UTC records a visible separate pointer at
(1602,618), type COLOR, 32x32, pitch 128, followed by successful frame upload.
cursor-check.png is a screenshot of the viewer; the cursor is visibly present
inside its captured desktop. This live check used the shared/clone desktop;
other pointer formats were verified by pixel-level unit tests.

API reference: https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api#updating-the-desktop-pointer

## Observed evidence

Source log: `display-debug-identity.log` (timestamps in UTC).
Validation was performed on the local Intel Iris Xe + Parsec VDD machine.

| Operation | Failure time (UTC Sep 5) | Actual HRESULT | First recovered frame | Recovery time |
| --- | --- | --- | --- | --- |
| Shared/clone desktop → extend | 16:32:55.326432 | 0x887A0026 ACCESS_LOST | 16:32:55.571729 | 245 ms |
| Five-second continuous timeout | 16:33:17.642383 | 0x887A0027 WAIT_TIMEOUT | 16:33:17.721642 | 79 ms after watchdog |
| Extend → clone | 16:33:42.109581 | 0x887A0026 ACCESS_LOST | 16:33:42.336555 | 227 ms |

Extension changed the owned Parsec monitor's GDI/DXGI name from DISPLAY1 to
DISPLAY57. The PnP instance stayed
`display\psccdd0\1&28a6823a&1&uid256`.
The DXGI output moved from index 0, primary, (0,0)-(1920,1080), to index 1,
non-primary, (1920,0)-(3840,1080). Capture used adapter index 0 / LUID (0,58104)
in both states. Clone restored DISPLAY1 / output 0 with another HMONITOR.

Logs continued beyond ten seconds after both changes, with subsequent frames
and successful heartbeat calls. Timeout watchdog recreation also repeated
successfully while the extended desktop was idle.

After normal window close, the process exited and `Get-PnpDevice -PresentOnly
-Class Monitor` showed only the integrated physical monitor. The display-adapter
inventory contained Intel Iris Xe and Parsec Virtual Display Adapter; no old
virtual-display adapter was reported.

## Confirmed code defects and changes

- Recovery previously constructed a new duplication while still holding the old
  one. It now drops the old object before creating another D3D11 device and
  duplication, including retries that fail. Field drop order releases duplication,
  staging and context before the device.
- HRESULTs are preserved and classified for acquisition/creation/release failures.
  Continuous WAIT_TIMEOUT triggers a five-second diagnostic watchdog.
- Acquisition now releases the frame even if the resource is missing or copying fails.
- Missing output during startup no longer disables all future recovery through
  an unset capture_display_name. Initial duplication failures enter recovery.
- Parsec heartbeat starts before monitor identification and continues through
  transient heartbeat errors.
- The dependency parsed only lowercase uid; the actual Windows registry path
  contains UID256. GDI/registry matching also needed case-insensitive comparison.
  The local patch fixes this and exposes the stable PnP instance path.
- Upstream adapter_instance is an empty-string stub and identifier is an
  enumeration ordinal. Neither is used as a persistent monitor identity.
- Recovery refreshes the owned PnP monitor and then matches its current name and
  HMONITOR to an attached DXGI output. Ambiguous/missing matches wait for retry.
  HMONITOR retains pointer width. Adapter LUID/output metadata are checked again
  during creation to reject topology races.
- RUST_LOG now controls the tracing filter.

The old startup.log does not contain the original failure HRESULT. Therefore,
the exact cause of the historical non-recovering state is not proven by an
old/new controlled comparison. The table records real failures and successful
recovery in the corrected implementation, rather than inferring success from
compilation alone.

## Interpretation and limits

Microsoft documents that DuplicateOutput can fail with E_INVALIDARG when the
same application is already duplicating the output, and requires the device to
belong to the output's adapter:
https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput

WAIT_TIMEOUT can occur on a healthy unchanged desktop. Its watchdog is a
conservative heuristic and periodically recreates capture during idle periods:
https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe

SESSION_DISCONNECTED and device removal/reset are classified but were not
reproduced. Multiple independent non-primary monitors and multiple Parsec
adapters were not available for hardware validation. Explicitly detached
outputs wait for reattachment; this change does not force Windows back into
extended mode or reinstall/remove display drivers. A clone shares its source
desktop and cannot provide an independent virtual workspace.

The placement call can fail when Windows initially attaches Parsec in clone
mode; that warning was observed. Explicit extension created the independent
1920x1080 output at (1920,0). Automatic clone-to-extend conversion remains outside
this recovery fix.

## Checks

- cargo check: passed after final Rust edits.
- cargo test -p parsec-vdd-rust --lib: UID regression passed.
- cargo build --release: passed (final build 1m 02s; only existing AppConfig/alpha dead-code warnings).

Final Release validation: display-debug-release.log records ACCESS_LOST at 16:38:40.805482 UTC and a recovered first frame plus renderer upload at 16:38:40.916025 UTC (111 ms). The owned PnP monitor changed DISPLAY57 -> DISPLAY1 and retained the same adapter LUID. No stderr output was recorded.

## Fullscreen and resolution settings (2026-09-06)

- Added five flush caption buttons: Settings, Fullscreen, Minimize, Maximize/Restore, Close.
- Native settings popup enumerates driver-supported resolutions, checks the current resolution, and preserves the current refresh rate when supported (otherwise 60 Hz or an available rate).
- Before applying, refresh the owned PnP identity and require exactly one active monitor on its GDI source. Real clone topology exposed both PSCCDD0 and BOE08FA on DISPLAY1; settings correctly disabled independent resolution changes.
- Apply with ChangeDisplaySettingsExW, first CDS_TEST then dynamic flags=0 (no global registry update). Failures report the actual DISP_CHANGE code to the user.
- Actual clicks in extended mode on DISPLAY57 changed 1920x1080 -> 1920x1200 -> 1920x1080. Test and apply both returned 0. New captured frames arrived 475 ms and 400 ms after apply respectively; window aspect ratio followed.
- Fullscreen occupied (0,0)-(1920,1080); returning restored (192,108)-(1728,972). Native edge hit testing returned HTCLIENT (1) while fullscreen.
- Reproduce GUI checks with settings-verify.ps1 -ViewerProcessId <pid>; results in settings-verification.json, settings-menu-items.json, and settings-debug.log.

## Separate settings window (2026-09-06)

The settings popup menu has been replaced with a modeless owned window in src/settings.rs. Window events are routed by WindowId, and native control commands are processed by App without stopping capture. Tab/Enter/Esc are routed through IsDialogMessageW. Font and child layout follow DPI changes. Only one settings window is opened at a time.

Verified with settings-window-verify.ps1: reuse of the same window; resolution Apply 1920x1080 -> 1920x1200 -> 1920x1080; Close button; reopen; Esc; title-bar close. The viewer stayed running. Both resolution changes returned DISP_CHANGE=0 and new frames arrived about 490 ms after apply. Restored the original clone topology afterward. Screenshot: settings-window.png; logs: settings-window-debug.log. This supersedes the earlier popup-menu verification scripts.

## Deferred settings confirmation (2026-09-06)

Settings content font reduced from 16 to 14 logical pixels. OK commits the selected resolution and optional view mode, then closes. Cancel, Esc and title-bar close discard the draft. Reopening the gear focuses the same settings window without resetting the draft. View mode defaults to keeping the current zoom/pan. Resolution errors leave the settings window open and do not apply the pending view change.

GUI verification: settings-confirm-verify.ps1; settings-confirm-debug.log; settings-confirm.png.

## Zoom toolbar and settings confirmation verification (2026-09-06)

- Settings native button commands now wake winit through EventLoopProxy; otherwise modeless child-control commands could remain queued until an unrelated viewer event.
- settings-confirm-verify.ps1 passed: deferred resolution selection, Cancel/X/Esc discard, repeated gear preserves draft, OK applies/closes, original resolution restored. Smaller 14-DIP content font verified in settings-confirm.png.
- zoom-ui-verify.ps1 exercised hover reveal, disabled actual ×4, 200% selection, slider to 400%, actual-size selection (125% at Fit scale 0.8), and fade-out. Captures in extended topology confirmed the hidden panel leaves no visible overlay. Zoom presets were also recorded in settings-confirm-debug.log. Clone topology was restored afterward.
- cargo check, 22 unit tests and cargo build --release passed. Tests cover actual-preset range checks at Fit scales 0.2/0.5/2, disabled-item interaction, slider endpoints, and short-window layout.

## Typography and slider thumb (2026-09-06)

Japanese zoom text now uses Yu Gothic UI, Latin runs use Aptos with a common baseline. Native settings controls use Yu Gothic UI for Japanese labels and Aptos for numeric/Latin controls. Microsoft official Aptos Regular/SemiBold were installed for the current user (not redistributed). Interactive-session font enumeration confirmed Aptos and Yu Gothic UI. Slider thumb now uses analytic pixel coverage for a smooth circular outline and accent center. cargo check and release build passed; zoom-ui-verify.ps1 exercised all existing controls and the new screenshot was inspected (zoom-menu.png). font-stderr.log was empty. Original clone topology restored.

## Windows system fonts only (2026-09-06)

Replaced Aptos with Segoe UI in zoom text and settings controls. Japanese remains Yu Gothic UI. Removed the downloaded Aptos archive and extracted files from target, and removed the README installation prerequisite. Neither the executable nor Inno Setup includes font files; the app only selects installed Windows system fonts.
