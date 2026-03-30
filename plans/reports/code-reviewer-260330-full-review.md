# Code Review — Deep Forge (2026-03-30)

## Critical Issues
1. `cached_source` extracted but never passed to `try_swap_frame_sync` — embedding cache is dead code
2. Profile photos response mismatch: backend returns `Vec<String>`, frontend expects `PhotoSlot[]`
3. `add_photo` response missing `photos` and `thumbnail_b64` fields editor expects
4. `delete_photo` same response mismatch

## Major Issues
5. `list_profiles` sorts by name, not creation time (comment says newest-first)
6. `url_suffix` naming misleading (actually serialized as full URL)
7. Can't distinguish FP16 vs FP32 inswapper loaded status
8. `provider_from_str` mapping undocumented
9. `activeId` lost on page reload
10. `list_profiles` sort uses wrong field

## Minor Issues
11. Dead `file?: string` field in types.ts
12. Duplicate `decode_to_bgr_frame` in router.rs and profiles.rs
13. Empty string sentinel for missing fallback_url
