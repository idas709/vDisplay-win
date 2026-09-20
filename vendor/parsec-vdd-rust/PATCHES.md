# Local fixes to parsec-vdd-rust 0.0.1

The upstream source and license are preserved here. Cargo uses this directory through [patch.crates-io].

- Parse monitor UID case-insensitively (Windows registry uses UID256).
- Compare GDI interface names and registry monitor paths case-insensitively.
- Expose the actual monitor PnP instance path for stable selection. Upstream adapter_instance is always an empty string; identifier is a generated enumeration ordinal.

Regression test: cargo test -p parsec-vdd-rust --lib

- Preserve full pointer width for HMONITOR instead of truncating it to i32.
