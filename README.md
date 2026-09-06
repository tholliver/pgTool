# pgTool (pgqb) — Build & Release Guide

## Prerequisites
- Rust toolchain installed (`rustup`)
- `CARGO_REGISTRY_TOKEN` only needed if publishing to crates.io (not required for binary distribution)

## `build.rs` (Windows icon embedding, cross-platform safe)
```rust
#[cfg(target_os = "windows")]
fn main() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app.ico");
    res.compile().expect("failed to compile Windows resources");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    // No-op on macOS/Linux — icon embedding only applies on Windows.
}
```

## Build Commands

### Windows
```bash
cargo build --release
```
Output: `target\release\pgqb.exe`

### macOS (Apple Silicon / M-series, incl. M5 Pro)
Build natively on the target Mac:
```bash
cargo build --release
```
Output: `target/release/pgqb`

Explicit target (CI or verification):
```bash
rustup target add aarch64-apple-darwin
cargo build --release --target aarch64-apple-darwin
```
Output: `target/aarch64-apple-darwin/release/pgqb`

> Cross-compiling to macOS from Windows/Linux is impractical (Apple SDK licensing). Build natively on macOS or use CI.

## CI: Build Both Platforms (GitHub Actions)
```yaml
name: Build Release
on: [push]
jobs:
  build:
    strategy:
      matrix:
        include:
          - os: windows-latest
          - os: macos-14   # Apple Silicon (M-series) runner
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - uses: actions/upload-artifact@v4
        with:
          name: pgqb-${{ matrix.os }}
          path: |
            target/release/pgqb
            target/release/pgqb.exe
```

## crates.io Publishing (only if publishing the crate itself, not the binary)
```bash
cargo login <your-api-token>
cargo publish
```
