# mimium-audio-plugin

This is a CLAP plugin built with Rust, Clack, and Wry. You edit mimium source code in a Monaco editor running in a webview, then compile and run it on the audio thread via the mimium-rs Wasm JIT backend.

## Structure

- `plugin/`: CLAP plugin implementation (Rust)
- `webview/`: UI built with Vite + React + Monaco
- `xtask/`: helper command for building packaged plugin artifacts
- `packaging/clap-wrapper/`: CMake configuration for clap-wrapper

## Development

1. Start the webview dev server:
   - `cd webview && pnpm dev`
2. Build the Rust side:
   - `cargo build`

In debug builds, the GUI points to `http://localhost:5173`.

## Packaging

Build `.clap`:

- `cargo run -p xtask -- package --release`

The output is placed at `target/package/release/mimium-audio-plugin.clap`.

Build `.vst3` via `clap-wrapper`:

- `cargo run -p xtask -- package --release --format vst3`

Build `.component` (AUv2) on macOS:

- `cargo run -p xtask -- package --release --format au`

Build `.clap` and `.vst3` together:

- `cargo run -p xtask -- package --release --all-formats`

On macOS, `--all-formats` builds `.clap`, `.vst3`, and `.component` together.

### clap-wrapper location

`xtask` looks for `clap-wrapper` in this order:

1. Environment variable `CLAP_WRAPPER_ROOT`
2. `third_party/clap-wrapper`

Example:

- `export CLAP_WRAPPER_ROOT=/path/to/clap-wrapper`

### Notes for loading in clap-host

- `clap-host` can load CLAP format only (not VST3).
- Use `target/package/release/mimium-audio-plugin.clap` as the file to load.
- `target/wrapper-stage/release/Mimium Audio Plugin.clap` is an internal staging bundle for clap-wrapper. Passing it to `clap-host` can cause `The shared library was not found.`.

## GitHub Actions Packaging

The installer build workflow is defined in [package-installers.yml](.github/workflows/package-installers.yml).

- Windows: builds `.msi` and installs into `Common Files/CLAP` and `Common Files/VST3`
- macOS: builds `.pkg` and installs into `/Library/Audio/Plug-Ins/CLAP`, `/Library/Audio/Plug-Ins/VST3`, and `/Library/Audio/Plug-Ins/Components`
- The macOS workflow signs when Apple certificates are provided, and runs notarize + staple when notarization secrets are configured

Secrets used for macOS signing and notarization:

- `APPLE_CERTIFICATES_P12_BASE64`
- `APPLE_CERTIFICATES_P12_PASSWORD`
- `APPLE_APPLICATION_SIGNING_IDENTITY`
- `APPLE_INSTALLER_SIGNING_IDENTITY`
- `APPLE_NOTARY_APPLE_ID`
- `APPLE_NOTARY_TEAM_ID`
- `APPLE_NOTARY_PASSWORD`

The Apple `.p12` is expected to include both `Developer ID Application` and `Developer ID Installer` certificates.