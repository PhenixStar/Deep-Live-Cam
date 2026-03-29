# Phase 5: Remote Server Mode

**Priority:** P2
**Effort:** 5h
**Status:** Pending

## Overview

Allow the Rust backend to accept remote connections so a user can send their webcam feed from another machine and receive the processed feed back.

## Architecture

```
Remote Client (browser/app)     LAN/Tailscale     Deep Forge Server
    ┌──────────────┐              │              ┌──────────────┐
    │ Upload source│──POST /source──────────────>│ dlc-server   │
    │ Connect WS   │──WS /ws/video──────────────>│ :8008        │
    │ View feed    │<──Binary JPEG frames────────│ (0.0.0.0)    │
    └──────────────┘              │              └──────────────┘
```

## Implementation Steps

### Step 1: Remote bind flag

Add `--remote` CLI flag to `dlc-server/src/main.rs`:
- Default: `127.0.0.1:8008` (localhost only)
- With `--remote`: `0.0.0.0:8008` (all interfaces)

### Step 2: API token authentication

Generate random UUID token on first remote-mode start. Store in `~/.deep-forge/api-token`.
Add middleware checking `X-Deep-Forge-Token` header. Skip auth for localhost connections.

### Step 3: CORS for remote

When `--remote`, add the remote client's origin to CORS (or use `allow_origin(Any)` with token auth).

### Step 4: UI toggle

"Server Mode" toggle in settings. When enabled:
- Restarts sidecar with `--remote` flag
- Shows LAN IP + port + API token
- Shows QR code for easy mobile connection (optional)

### Step 5: Connection status

Show connected remote clients count in metrics panel.

## Todo

- [ ] Add `--remote` CLI flag
- [ ] Generate and store API token
- [ ] Add auth middleware
- [ ] Update CORS for remote mode
- [ ] Add server mode toggle in UI
- [ ] Display LAN IP + token when remote enabled

## Success Criteria

- Remote browser can connect and view processed feed
- API token required for non-localhost connections
- UI clearly shows when server mode is active
