# Tauri v2 Cross-Platform Installers Research

**Date:** 2026-03-28 | **Status:** Complete

## 1. Bundle Configuration (tauri.conf.json)

**Supported formats:** MSI, NSIS (Windows); DMG (macOS); AppImage, DEB, RPM (Linux).

Configure in `bundle` section:
- `targets`: array of ["msi", "dmg", "appimage"] or "all"
- `windows`: MSI-specific settings (icon, license file, installer UI)
- `macOS`: DMG settings (background, icon layout, codesigning identity)
- `linux`: AppImage settings (app image format, desktop integration)

**Output:** `target/release/bundle/` with platform subdirectories (nsis/, msi/, dmg/, appimage/).

**Key setting:** `externalBin` array for sidecar binaries with platform-specific naming: `binaries/app-x86_64-pc-windows-msvc`, `binaries/app-aarch64-apple-darwin`, etc.

## 2. GitHub Actions CI/CD

**Tool:** `tauri-apps/tauri-action` automates build & release for all platforms.

**Matrix strategy:**
```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
    arch: [x86_64, aarch64]  # macOS Intel + Apple Silicon
```

**Features:**
- Parallel cross-platform builds
- Artifact upload to GitHub Releases
- Generates `latest.json` for auto-updates
- Platform-specific dependencies auto-handled (libwebkit2gtk-4.1-dev on Linux)

**New:** Aug 2025, GitHub released ARM64 runners (ubuntu-22.04-arm, ubuntu-24.04-arm) for public repos.

## 3. Code Signing

### macOS (Developer ID Application)
1. Generate Certificate Signing Request (CSR) → import to Keychain
2. Tauri signs binary with `codesign`
3. **Notarization required** — Apple's automated security check:
   - Upload signed app to Apple servers
   - If approved, Apple staples "ticket" to app
   - Users won't see security warnings
4. Authenticate via App Store Connect API or Apple ID (set env vars in CI)

### Windows (Authenticode)
1. Acquire code signing cert (Digicert, Sectigo, GoDaddy)
2. **Extended Validation (EV) cert** eliminates installer prompts
3. Use `bundle.windows.signCommand` for custom signing tools
4. Set cert path & password in CI/CD env vars

### Linux
No signing required. AppImage distributable as-is.

## 4. Auto-Update Mechanism

**Plugin:** `tauri-plugin-updater` supports two backends:

### Static JSON (GitHub Releases)
- tauri-action generates `latest.json` automatically
- Endpoint: `https://github.com/<user>/<repo>/releases/latest/download/latest.json`
- JSON structure: `{version, platforms.{target}.url, platforms.{target}.signature}`
- No backend server needed

### Dynamic Backend
- Use FastAPI or similar to host update info
- App queries server on startup for latest version
- More complex but allows A/B testing, gradual rollouts

**Workflow:** Commit → GitHub Actions builds → uploads to Release → `latest.json` updated → App checks & downloads.

## 5. Sidecar Binaries (Python Backend)

**Use case:** Bundle Python backend without requiring user Python installation.

**Setup:**
1. Create platform-specific binaries with PyInstaller: `app-x86_64-pc-windows-msvc`, `app-aarch64-apple-darwin`, etc.
2. Place in `src-tauri/binaries/`
3. Configure in `tauri.conf.json`: `"externalBin": ["binaries/app"]`
4. Grant execute permission in `src-tauri/capabilities/default.json`

**Execution:** Call `sidecar()` with just filename, not full path. Tauri auto-selects platform binary.

**Bundling:** All platform binaries included in release; Tauri extracts & runs the matching architecture on app launch.

## Unresolved Questions

- DMG signing: Does macOS notarization apply to DMG files or just the .app inside?
- Sidecar permission model: Can sidecar communicate back to frontend? Are there IPC limitations?
- Auto-update security: How to validate `latest.json` signature before trusting?
- Windows EV cert cost vs. standard cert trade-off guidance?

---

**Sources:**
- [Tauri v2 Configuration](https://v2.tauri.app/reference/config/)
- [Tauri GitHub Actions Pipeline](https://v2.tauri.app/distribute/pipelines/github/)
- [tauri-action Repository](https://github.com/tauri-apps/tauri-action)
- [macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)
- [Windows Code Signing](https://v2.tauri.app/distribute/sign/windows/)
- [Updater Plugin](https://v2.tauri.app/plugin/updater/)
- [Sidecar Binaries](https://v2.tauri.app/develop/sidecar/)
- [Auto-Updates with GitHub](https://thatgurjot.com/til/tauri-auto-updater/)
- [Production-Ready Tauri + FastAPI](https://aiechoes.substack.com/p/building-production-ready-desktop)
