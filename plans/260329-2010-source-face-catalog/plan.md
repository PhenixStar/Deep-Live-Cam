---
title: "Source Face Profile Catalog"
description: "Replace plain image upload with multi-photo face profiles and catalog management"
status: complete
priority: P1
effort: 12h
branch: main
tags: [face-profile, catalog, ux, embedding-cache]
created: 2026-03-29
---

# Source Face Profile Catalog

## Overview

Replace the current single-image source face upload with a **profile-based catalog system**. Users create face profiles from multiple photos (up to 6), which generates an averaged face embedding for more accurate and consistent swaps.

## User Flow

```
Main UI
  ├── Source dropdown: [Profile 1 ▾] [Profile 2] [+ Add New]
  │         │
  │         └── Click "Add New" or profile name
  │                  │
  │                  ▼
  ├── Profile Catalog (Modal Popup)
  │     ├── Grid of saved profiles (thumbnail + name + score)
  │     ├── [Create New Profile] button
  │     ├── Click profile → Edit view
  │     └── Click "Use" → select for swap
  │                  │
  │                  ▼
  └── Profile Editor (inside catalog modal)
        ├── LEFT:  6 upload slots (drag & drop or file picker)
        │          Each shows uploaded photo with face detection overlay
        │          Green border = face detected, Red = no face
        ├── RIGHT: Mapped composite face preview
        │          Generated from averaging detected faces across uploads
        ├── Name field (required)
        ├── Description field (optional)
        ├── [Save] [Delete] [Cancel] buttons
        └── Score indicator (avg detection confidence)
```

## Architecture

### Backend (Rust dlc-server)

```
GET    /profiles              → list all profiles (id, name, thumbnail_b64, score)
POST   /profiles              → create profile (name, description)
GET    /profiles/{id}         → get profile detail (photos, embedding, metadata)
PUT    /profiles/{id}         → update name/description
DELETE /profiles/{id}         → delete profile + associated files
POST   /profiles/{id}/photos  → upload photo (multipart), detect face, add to profile
DELETE /profiles/{id}/photos/{idx} → remove photo from profile
POST   /profiles/{id}/activate → set as active source for swapping
```

### Storage Layout

```
models/profiles/
├── {uuid}/
│   ├── meta.json          # {name, description, created, score}
│   ├── embedding.bin      # 512-dim f32 averaged embedding (2KB)
│   ├── thumbnail.jpg      # Composite face thumbnail (128x128)
│   ├── photo_0.jpg        # Original upload
│   ├── photo_1.jpg
│   └── ...
```

### Embedding Strategy

1. Each uploaded photo → detect face → extract ArcFace embedding (512-dim)
2. Average all embeddings → L2-normalize → stored as `embedding.bin`
3. More photos = more robust embedding (covers angles, lighting)
4. Minimum 1 photo required, maximum 6

### Frontend Components

```
app/src/components/
├── profile-catalog.tsx     # Modal with profile grid
├── profile-editor.tsx      # Create/edit view with 6 upload slots
├── profile-card.tsx        # Grid item: thumbnail + name + score
└── source-selector.tsx     # Dropdown replacing plain file input
```

## Phases

| Phase | What | Effort |
|-------|------|--------|
| 1 | Backend CRUD: /profiles endpoints, storage, embedding | 4h |
| 2 | Frontend catalog modal + profile grid | 4h |
| 3 | Profile editor with 6-slot upload + composite preview | 3h |
| 4 | Wire source dropdown + activate on swap | 1h |

## Phase 1: Backend CRUD

### State additions (state.rs)
```rust
pub profiles_dir: PathBuf,  // models/profiles/
```

### New module: `dlc-server/src/profiles.rs`
- `Profile` struct: id, name, description, photos (Vec<PathBuf>), embedding (Option<Vec<f32>>), score
- CRUD handlers for axum
- Photo upload → detect face → extract embedding → update average
- Thumbnail generation: align best-scoring face → 128x128 JPEG

### Embedding computation
```rust
fn compute_averaged_embedding(photos: &[PathBuf], detector: &FaceDetector, swapper: &FaceSwapper) -> Result<Vec<f32>> {
    let mut embeddings = Vec::new();
    for photo in photos {
        let frame = load_image(photo)?;
        let faces = detector.detect(&frame, 0.5)?;
        if let Some(face) = faces.first() {
            let emb = swapper.get_embedding(&frame, face)?;
            embeddings.push(emb);
        }
    }
    // Average + L2 normalize
    let avg = average_vectors(&embeddings)?;
    Ok(l2_normalize(&avg))
}
```

## Phase 2: Catalog Modal

### profile-catalog.tsx
- Full-screen modal overlay
- Grid of profile cards (3 columns)
- Search/filter by name
- "Create New" button
- Click card → opens editor OR "Use" button activates

### profile-card.tsx
- Thumbnail (128x128)
- Profile name
- Photo count badge (e.g., "4/6 photos")
- Confidence score bar
- "Use" and "Edit" action buttons

## Phase 3: Profile Editor

### profile-editor.tsx
- Left panel: 6 photo upload slots (2x3 grid)
  - Each slot: drag-drop zone or click-to-upload
  - After upload: shows photo with face bbox overlay
  - Green border = face detected (score shown)
  - Red border = no face detected (allows retry)
  - X button to remove photo
- Right panel: composite face preview
  - Shows best-aligned face from uploads
  - Updates live as photos are added
- Bottom: name input, description textarea, save/delete/cancel

## Phase 4: Source Selector

### source-selector.tsx (replaces file input in controls-panel.tsx)
```tsx
<div className="source-selector">
  <label>Source Face</label>
  <select value={activeProfile} onChange={handleProfileChange}>
    {profiles.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
    <option value="__new__">+ Add New Profile</option>
  </select>
  {activeProfile && <img src={profileThumbnail} className="face-preview" />}
</div>
```

## Success Criteria

- [ ] User can create a profile with 1-6 photos
- [ ] Face detection runs on each uploaded photo with visual feedback
- [ ] Averaged embedding produces better swap quality than single photo
- [ ] Profiles persist across app restarts (stored in models/profiles/)
- [ ] Activating a profile instantly switches the source face (cached embedding)
- [ ] Catalog modal responsive and smooth on all platforms
- [ ] Delete profile removes all associated files

## CCS Delegation

| Phase | Task | Agent |
|-------|------|-------|
| 1 | Backend CRUD endpoints | mmu |
| 1 | Embedding computation | Claude (math) |
| 2 | Catalog modal + grid | mmu |
| 3 | Editor with upload slots | mmu |
| 4 | Source selector dropdown | mmu |
