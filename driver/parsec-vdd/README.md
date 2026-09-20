# Parsec VDD runtime files

This directory contains the Parsec VDD portable package used by the installer.
The package includes the driver setup executable and the Parsec VDD control
executable:

```text
parsec-vdd/
├── parsec-vdd-0.45.0.0.exe
└── ParsecVDisplay.exe
```

The current Rust runtime uses the installed Parsec VDD device through
`parsec-vdd-rust`. Setup embeds only `parsec-vdd-0.45.0.0.exe` and runs it with `/S`
when no usable registered installation exists. `ParsecVDisplay.exe` is not shipped.
The checked setup has a valid Parsec Cloud, Inc. Authenticode signature and SHA-256:

`E23332448FDAF5AA017CB308DB5EF6855FAC526A7DED05D80C039404126D5362`

The app adds/removes virtual displays only while running. During application
uninstall, users can remove the shared driver (default) or retain it. Removal uses
the vendor's registered `ParsecVDD` uninstaller, not broad device deletion.
See `installer/README.md` for build and verification instructions.
