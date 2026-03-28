# Phase 3: Cross-Platform Installers + CI/CD

**Effort:** 4h
**Team:** Team 3 (DevOps)
**Depends on:** Phase 2 (needs bundled sidecar to package)

---

## Problem

No automated build pipeline exists. Developers must manually run `pnpm tauri build` on each platform. No installers, no code signing, no auto-update mechanism.

## Goal

GitHub Actions workflow that builds MSI (Windows), DMG (macOS), and AppImage (Linux) on tag push, publishes to GitHub Releases, and supports Tauri's auto-update plugin.

## Current State

- No `.github/workflows/` directory exists
- `tauri.conf.json` has `"targets": "all"` (builds all formats) and bundle icons configured
- `package.json` uses `pnpm@10.29.3` as package manager
- Rust edition 2021, Tauri v2 with `tauri-plugin-shell`
- No `tauri-plugin-updater` dependency yet

---

## Architecture

```
Tag push (v*)
  |
  v
GitHub Actions Matrix:
  +-- ubuntu-latest  --> build-sidecar.sh  --> pnpm tauri build --> AppImage + .deb
  +-- macos-latest   --> build-sidecar-macos.sh --> pnpm tauri build --> DMG
  +-- windows-latest --> build-sidecar-win.ps1  --> pnpm tauri build --> MSI + NSIS
  |
  v
GitHub Release:
  +-- deep-live-cam_0.1.0_amd64.AppImage
  +-- deep-live-cam_0.1.0_amd64.AppImage.sig  (for auto-update)
  +-- Deep-Live-Cam_0.1.0_x64_en-US.msi
  +-- Deep-Live-Cam_0.1.0_x64_en-US.msi.sig
  +-- Deep-Live-Cam_0.1.0_aarch64.dmg
  +-- Deep-Live-Cam_0.1.0_aarch64.dmg.sig
  +-- latest.json  (updater manifest)
```

---

## Implementation Steps

### Step 1: Add `tauri-plugin-updater` dependency

**Rust side (`src-tauri/Cargo.toml`):**
```toml
[dependencies]
tauri-plugin-updater = "2"
```

**JS side (`package.json`):**
```json
"dependencies": {
  "@tauri-apps/plugin-updater": "^2"
}
```

**Register plugin in `main.rs`:**
```rust
tauri::Builder::default()
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    // ...
```

**CCS Delegation:** mmu agent -- dependency additions.

### Step 2: Configure updater in `tauri.conf.json`

Add updater endpoint and signing pubkey:

```json
{
  "plugins": {
    "updater": {
      "endpoints": [
        "https://github.com/phenixstar/deep-live-cam-app/releases/latest/download/latest.json"
      ],
      "pubkey": "UPDATER_PUBKEY_PLACEHOLDER"
    }
  }
}
```

The pubkey is generated via `pnpm tauri signer generate -w ~/.tauri/deep-live-cam.key`. The private key becomes a GitHub Actions secret.

**CCS Delegation:** mmu agent for config. Team 3 Leader generates the signing keypair.

### Step 3: Generate Tauri updater signing keys

```bash
pnpm tauri signer generate -w ~/.tauri/deep-live-cam.key
```

Outputs:
- `~/.tauri/deep-live-cam.key` -- private key (store as GitHub secret `TAURI_SIGNING_PRIVATE_KEY`)
- `~/.tauri/deep-live-cam.key.pub` -- public key (embed in `tauri.conf.json`)
- Password prompt -- store as GitHub secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

**CCS Delegation:** Team 3 Leader -- security-sensitive operation.

### Step 4: Configure bundle targets per platform

Update `tauri.conf.json` bundle section:

```json
{
  "bundle": {
    "active": true,
    "targets": ["msi", "nsis", "dmg", "appimage", "deb"],
    "resources": ["sidecar/**/*"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "externalBin": ["binaries/deep-live-cam-server"],
    "windows": {
      "certificateThumbprint": null,
      "digestAlgorithm": "sha256",
      "timestampUrl": ""
    },
    "macOS": {
      "minimumSystemVersion": "11.0",
      "signingIdentity": null
    },
    "linux": {
      "deb": {
        "depends": ["libwebkit2gtk-4.1-0", "libssl3"]
      }
    }
  }
}
```

Notes:
- `certificateThumbprint` and `signingIdentity` are null for v1 (optional signing)
- `resources` includes the sidecar directory from Phase 2
- Linux deb needs webkit2gtk and openssl as system deps

**CCS Delegation:** mmu agent.

### Step 5: Create GitHub Actions workflow

**`.github/workflows/release.yml`:**

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

env:
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-22.04
            target: x86_64-unknown-linux-gnu
            sidecar-script: scripts/build-sidecar.sh
          - os: macos-14
            target: aarch64-apple-darwin
            sidecar-script: scripts/build-sidecar-macos.sh
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            sidecar-script: scripts/build-sidecar-win.ps1

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 10

      - name: Install Node.js
        uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
          cache-dependency-path: deep-live-cam-app/pnpm-lock.yaml

      # Linux-specific system deps
      - name: Install Linux dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev libappindicator3-dev \
            librsvg2-dev patchelf libssl-dev \
            zstd

      # macOS: install zstd for python-build-standalone extraction
      - name: Install macOS dependencies
        if: runner.os == 'macOS'
        run: brew install zstd

      # Build the Python sidecar
      - name: Build sidecar (Unix)
        if: runner.os != 'Windows'
        run: bash ${{ matrix.sidecar-script }}

      - name: Build sidecar (Windows)
        if: runner.os == 'Windows'
        run: pwsh ${{ matrix.sidecar-script }}

      # Install frontend deps and build Tauri app
      - name: Install frontend dependencies
        working-directory: deep-live-cam-app
        run: pnpm install --frozen-lockfile

      - name: Build Tauri app
        uses: tauri-apps/tauri-action@v0
        with:
          projectPath: deep-live-cam-app
          tagName: ${{ github.ref_name }}
          releaseName: "Deep Live Cam ${{ github.ref_name }}"
          releaseBody: "See the assets below to download and install."
          releaseDraft: true
          prerelease: false
          includeUpdaterJson: true
```

**Key decisions:**
- `fail-fast: false` -- one platform failure shouldn't block others
- `releaseDraft: true` -- allows manual review before publishing
- `includeUpdaterJson: true` -- `tauri-action` auto-generates `latest.json` for the updater
- `ubuntu-22.04` pinned (not `latest`) for reproducible webkit2gtk version
- `macos-14` for Apple Silicon runners (M1+)

**CCS Delegation:** Team 3 Leader writes initial workflow. mmu agents handle per-platform dep installation blocks.

### Step 6: Add frontend auto-update check

Add update check on app startup in `App.tsx` or a new `Updater.tsx` component:

```typescript
import { check } from "@tauri-apps/plugin-updater";

async function checkForUpdates() {
  try {
    const update = await check();
    if (update) {
      const confirmed = window.confirm(
        `Update ${update.version} available. Download now?`
      );
      if (confirmed) {
        await update.downloadAndInstall();
        // App will restart on next launch
      }
    }
  } catch (e) {
    console.warn("Update check failed:", e);
  }
}
```

Call `checkForUpdates()` in a `useEffect` on mount with a 5-second delay (don't block app startup).

**CCS Delegation:** mmu agent.

### Step 7: Add capabilities for updater plugin

Create or update `src-tauri/capabilities/default.json`:

```json
{
  "identifier": "default",
  "description": "Default app capabilities",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-spawn",
    "shell:allow-execute",
    "updater:default"
  ]
}
```

**CCS Delegation:** mmu agent.

### Step 8: Add dev workflow (PR checks, no release)

**`.github/workflows/ci.yml`:**

```yaml
name: CI

on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: pnpm/action-setup@v4
        with:
          version: 10

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
          cache-dependency-path: deep-live-cam-app/pnpm-lock.yaml

      - name: Install Linux dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev libappindicator3-dev \
            librsvg2-dev patchelf libssl-dev

      - name: Frontend lint + typecheck
        working-directory: deep-live-cam-app
        run: |
          pnpm install --frozen-lockfile
          pnpm build

      - name: Rust check
        working-directory: deep-live-cam-app/src-tauri
        run: cargo check
```

Intentionally lightweight: no sidecar build on PRs (saves 10+ min). Validates that frontend compiles and Rust compiles.

**CCS Delegation:** mmu agent.

---

## Code Signing (Optional for v1)

### Windows Code Signing

Requires an EV code signing certificate (~$200-400/yr). Without it, users see SmartScreen warning ("Windows protected your PC").

**Setup if certificate available:**
1. Store certificate as GitHub secret `WINDOWS_CERTIFICATE` (base64 PFX)
2. Store password as `WINDOWS_CERTIFICATE_PASSWORD`
3. Add to `tauri.conf.json`: `"certificateThumbprint": "<thumbprint>"`
4. `tauri-action` handles signing automatically

**v1 decision:** Skip. Document the SmartScreen bypass for early adopters.

### macOS Code Signing + Notarization

Requires Apple Developer account ($99/yr). Without it, users must right-click > Open to bypass Gatekeeper.

**Setup if account available:**
1. Export signing identity to GitHub secrets
2. Add env vars: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`
3. `tauri-action` handles signing + notarization

**v1 decision:** Skip. Document the `xattr -cr` workaround.

### Linux

No code signing needed. AppImage and .deb are unsigned by convention.

---

## CCS Delegation Map

| Step | Task | Assignee | Rationale |
|------|------|----------|-----------|
| 1 | Add updater plugin deps | mmu | Dependency additions |
| 2 | Configure updater in tauri.conf.json | mmu | Config change |
| 3 | Generate signing keypair | **Team 3 Leader** | Security-sensitive |
| 4 | Configure bundle targets | mmu | Config JSON |
| 5 | Create release.yml workflow | **Team 3 Leader** | Core deliverable |
| 6 | Frontend update check component | mmu | Standard Tauri plugin usage |
| 7 | Add capabilities JSON | mmu | Config file |
| 8 | Create ci.yml workflow | mmu | Simpler version of release.yml |

---

## Success Criteria

- [ ] `git tag v0.1.0 && git push --tags` triggers GitHub Actions build on all 3 platforms
- [ ] Linux job produces `.AppImage` and `.deb` artifacts
- [ ] macOS job produces `.dmg` artifact
- [ ] Windows job produces `.msi` artifact
- [ ] All artifacts uploaded to a draft GitHub Release
- [ ] `latest.json` updater manifest is generated and uploaded
- [ ] PR checks (ci.yml) pass: frontend typecheck + Rust cargo check
- [ ] App installed from AppImage on Linux launches successfully with bundled sidecar
- [ ] Update check on app startup does not block UI or crash if offline

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| GitHub Actions runners lack CUDA | Cannot test GPU inference in CI | CI validates build only; GPU testing is manual QA |
| Sidecar build takes >30 min | CI timeout | Cache python-build-standalone download with `actions/cache`; set job timeout to 60 min |
| macOS runner architecture mismatch | Wrong DMG arch | Pin `macos-14` for ARM64; add `macos-13` matrix entry for x86_64 if needed |
| `tauri-action` version incompatible with Tauri v2 | Build fails | Pin `tauri-apps/tauri-action@v0` (v0 supports Tauri v2) |
| Unsigned binaries trigger OS warnings | Poor first-run UX | Document workarounds in release notes; add signing in v2 |
| Sidecar directory too large for GitHub Release (2 GB limit) | Upload fails | Compress sidecar; exclude models (downloaded at first run) |
| `pnpm-lock.yaml` missing or stale | `--frozen-lockfile` fails | Ensure lock file committed before tagging |

## Unresolved Questions

1. Should we add a `macos-13` (Intel) matrix entry for x86_64 DMG, or ship ARM64-only for now?
2. What's the GitHub repo URL for the updater endpoint? Need to confirm `phenixstar/deep-live-cam-app` or a different org/repo.
3. Should draft releases auto-publish, or require manual approval? (Current plan: draft, manual publish.)
4. Do we need a separate `nightly` workflow on main branch pushes for pre-release builds?
