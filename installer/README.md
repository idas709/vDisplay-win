# Installer / uninstaller

Build from the repository root:

```powershell
.\installer\build.ps1
# Or specify your Inno Setup compiler:
.\installer\build.ps1 -Iscc 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
```

Requires Inno Setup 6.7+ and the Rust MSVC build environment. The script checks
the Parsec setup's Authenticode signature, runs `cargo check` and
`cargo build --release`, compiles the installer, and writes its SHA-256 checksum.

Output: `target\installer\VirtualDisplayWorkspace-Setup.exe`.

## Installation

- Windows 10 21H2 or later, x64-compatible system; administrator permission required.
- Japanese and English setup UI.
- Installs the app and Windows uninstall registration, plus shortcuts.
- Reuses an existing registered Parsec VDD with its vendor uninstaller present.
- Otherwise extracts the signed `parsec-vdd-0.45.0.0.exe` and runs `/S` before
  installing the app. Failure stops setup; exit codes and registration are checked.
- Only the app and the signed vendor setup are included. `ParsecVDisplay.exe`
  and font files are not bundled.
- The viewer holds `Local\VirtualDisplayWorkspace.App`; setup/uninstall asks for
  running viewers to be closed before continuing.

## Uninstallation

Use Windows **Installed apps → Virtual Display Workspace → Uninstall**, or
`unins000.exe` in the installation folder. Inno Setup creates this uninstaller
during installation; it is not a separate download.

The driver question defaults to **Yes**:

- **Yes**: remove the app and invoke Parsec VDD's registered vendor uninstaller.
- **No**: remove the app while keeping the shared Parsec VDD installation.
- **Cancel**: cancel the application uninstall without removing either component.

Driver removal happens only after the application's final uninstall confirmation.
The vendor NSIS uninstaller is copied to a temporary directory and called with
`/S _?=<registered installation directory>` so the parent waits for it. A failed
driver uninstall is reported; app removal continues, and the driver can still be
removed through Windows Installed apps. Other virtual display drivers and the
Parsec application itself are not targeted. User preferences are retained.

Silent examples (from an elevated terminal):

```powershell
.\VirtualDisplayWorkspace-Setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="setup.log"
# Remove the app and VDD (default):
.\unins000.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="uninstall.log"
# Keep VDD:
.\unins000.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /KEEPVDD /LOG="uninstall.log"
```

## Verification

Built successfully using Inno Setup 6.7.3, after cargo check and release build.
Confirmed the local driver's registered `ParsecVDD` key, installation directory,
vendor uninstall batch commands and valid Parsec Cloud signatures on both setup
and uninstaller. The current environment was not subjected to elevated
installation or actual driver removal.

Before distribution, test on an expendable Windows machine: fresh installation,
existing-driver reuse, upgrade while the viewer is open, and uninstall with
Yes / No / Cancel. Check exit codes and logs, and confirm driver/device removal
only on Yes. The generated app installer is unsigned unless you configure signing.
