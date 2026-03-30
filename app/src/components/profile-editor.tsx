import { useState, useEffect, useRef, useCallback } from "react";

const API_BASE = "http://localhost:8008";

// ── Types ────────────────────────────────────────────────────────────────────

interface PhotoSlot {
  url: string;
  score: number;
  has_face: boolean;
}

interface ProfileDetail {
  id: string;
  name: string;
  description: string;
  photos: PhotoSlot[];
  thumbnail_b64: string | null;
}

export interface ProfileEditorProps {
  profileId: string | null; // null = create new
  onSave: () => void;
  onCancel: () => void;
}

// ── Component ────────────────────────────────────────────────────────────────

const MAX_SLOTS = 6;

export function ProfileEditor({ profileId, onSave, onCancel }: ProfileEditorProps) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [photos, setPhotos] = useState<(PhotoSlot | null)[]>(Array(MAX_SLOTS).fill(null));
  const [thumbnail, setThumbnail] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [uploading, setUploading] = useState<boolean[]>(Array(MAX_SLOTS).fill(false));
  const [error, setError] = useState<string | null>(null);
  // profileId resolved after creation (null → new id after POST /profiles)
  const [resolvedId, setResolvedId] = useState<string | null>(profileId);
  // drag-over slot index
  const [dragOver, setDragOver] = useState<number | null>(null);

  const nameRef = useRef<HTMLInputElement>(null);

  // ── Load existing profile ──────────────────────────────────────────────────
  useEffect(() => {
    if (!profileId) {
      // Create mode: autofocus name field
      nameRef.current?.focus();
      return;
    }
    setResolvedId(profileId);
    fetch(`${API_BASE}/profiles/${profileId}`)
      .then((r) => r.json())
      .then((data: ProfileDetail) => {
        setName(data.name);
        setDescription(data.description ?? "");
        setThumbnail(
          data.thumbnail_b64 ? `data:image/jpeg;base64,${data.thumbnail_b64}` : null,
        );
        // Pad photos array to MAX_SLOTS with nulls
        // Backend may return photos as plain URL strings or as PhotoSlot objects
        const slots: (PhotoSlot | null)[] = Array(MAX_SLOTS).fill(null);
        (data.photos ?? []).forEach((p: PhotoSlot | string, i: number) => {
          if (i < MAX_SLOTS) {
            slots[i] = typeof p === "string" ? { url: p, score: 0, has_face: true } : p;
          }
        });
        setPhotos(slots);
      })
      .catch(() => setError("Failed to load profile"));
  }, [profileId]);

  // ── Ensure profile exists before uploading ────────────────────────────────
  const ensureProfile = useCallback(async (): Promise<string | null> => {
    if (resolvedId) return resolvedId;
    if (!name.trim()) {
      setError("Enter a name before uploading photos");
      nameRef.current?.focus();
      return null;
    }
    try {
      const res = await fetch(`${API_BASE}/profiles`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: name.trim(), description: description.trim() }),
      });
      if (!res.ok) throw new Error(await res.text());
      const data = (await res.json()) as { id: string };
      setResolvedId(data.id);
      return data.id;
    } catch (e) {
      setError(`Failed to create profile: ${e instanceof Error ? e.message : String(e)}`);
      return null;
    }
  }, [resolvedId, name, description]);

  // ── Upload a file to a slot ───────────────────────────────────────────────
  const uploadFile = useCallback(
    async (slotIndex: number, file: File) => {
      if (uploading[slotIndex]) return;

      const id = await ensureProfile();
      if (!id) return;

      setUploading((prev) => {
        const next = [...prev];
        next[slotIndex] = true;
        return next;
      });
      setError(null);

      try {
        const fd = new FormData();
        fd.append("file", file);
        const res = await fetch(`${API_BASE}/profiles/${id}/photos`, {
          method: "POST",
          body: fd,
        });
        if (!res.ok) throw new Error(await res.text());

        // Re-fetch profile detail to get the authoritative updated photo list
        const detailRes = await fetch(`${API_BASE}/profiles/${id}`);
        if (detailRes.ok) {
          const data = await detailRes.json();
          const slots: (PhotoSlot | null)[] = Array(MAX_SLOTS).fill(null);
          (data.photos ?? []).forEach((p: PhotoSlot | string, i: number) => {
            if (i < MAX_SLOTS) {
              slots[i] = typeof p === "string" ? { url: p, score: 0, has_face: true } : p;
            }
          });
          setPhotos(slots);
          if (data.thumbnail_b64) setThumbnail(`data:image/jpeg;base64,${data.thumbnail_b64}`);
        }
      } catch (e) {
        setError(`Upload failed: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        setUploading((prev) => {
          const next = [...prev];
          next[slotIndex] = false;
          return next;
        });
      }
    },
    [uploading, ensureProfile],
  );

  // ── Remove photo ──────────────────────────────────────────────────────────
  const removePhoto = useCallback(
    async (slotIndex: number) => {
      if (!resolvedId) return;
      try {
        const res = await fetch(
          `${API_BASE}/profiles/${resolvedId}/photos/${slotIndex}`,
          { method: "DELETE" },
        );
        if (!res.ok) throw new Error(await res.text());

        // Re-fetch profile detail to get the authoritative updated photo list
        const detailRes = await fetch(`${API_BASE}/profiles/${resolvedId}`);
        if (detailRes.ok) {
          const data = await detailRes.json();
          const slots: (PhotoSlot | null)[] = Array(MAX_SLOTS).fill(null);
          (data.photos ?? []).forEach((p: PhotoSlot | string, i: number) => {
            if (i < MAX_SLOTS) {
              slots[i] = typeof p === "string" ? { url: p, score: 0, has_face: true } : p;
            }
          });
          setPhotos(slots);
          setThumbnail(data.thumbnail_b64 ? `data:image/jpeg;base64,${data.thumbnail_b64}` : null);
        }
      } catch (e) {
        setError(`Remove failed: ${e instanceof Error ? e.message : String(e)}`);
      }
    },
    [resolvedId],
  );

  // ── Save metadata ─────────────────────────────────────────────────────────
  const handleSave = useCallback(async () => {
    if (!name.trim()) {
      setError("Name is required");
      nameRef.current?.focus();
      return;
    }
    setSaving(true);
    setError(null);
    try {
      let id = resolvedId;
      if (!id) {
        // No photos uploaded yet — create profile now
        id = await ensureProfile();
        if (!id) return;
      }
      const res = await fetch(`${API_BASE}/profiles/${id}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: name.trim(), description: description.trim() }),
      });
      if (!res.ok) throw new Error(await res.text());
      onSave();
    } catch (e) {
      setError(`Save failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  }, [name, description, resolvedId, ensureProfile, onSave]);

  // ── Delete profile ────────────────────────────────────────────────────────
  const handleDelete = useCallback(async () => {
    if (!resolvedId) { onCancel(); return; }
    if (!window.confirm("Delete this profile and all its photos?")) return;
    try {
      const res = await fetch(`${API_BASE}/profiles/${resolvedId}`, { method: "DELETE" });
      if (!res.ok) throw new Error(await res.text());
      onSave(); // close and refresh parent
    } catch (e) {
      setError(`Delete failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }, [resolvedId, onSave, onCancel]);

  // ── File input handler ────────────────────────────────────────────────────
  const handleFileInput = useCallback(
    (slotIndex: number) => (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (file) void uploadFile(slotIndex, file);
      // Reset input value so same file can be re-selected
      e.target.value = "";
    },
    [uploadFile],
  );

  // ── Drag & drop handlers ──────────────────────────────────────────────────
  const handleDragOver = useCallback(
    (slotIndex: number) => (e: React.DragEvent) => {
      e.preventDefault();
      setDragOver(slotIndex);
    },
    [],
  );

  const handleDragLeave = useCallback(() => {
    setDragOver(null);
  }, []);

  const handleDrop = useCallback(
    (slotIndex: number) => (e: React.DragEvent) => {
      e.preventDefault();
      setDragOver(null);
      const file = e.dataTransfer.files[0];
      if (file && file.type.startsWith("image/")) {
        void uploadFile(slotIndex, file);
      }
    },
    [uploadFile],
  );

  // ── Compute avg score for display ─────────────────────────────────────────
  const filledSlots = photos.filter(Boolean) as PhotoSlot[];
  const avgScore =
    filledSlots.length > 0
      ? filledSlots.reduce((s, p) => s + p.score, 0) / filledSlots.length
      : null;

  // ── Render ────────────────────────────────────────────────────────────────
  return (
    <div className="profile-editor">
      {/* ── Left panel: 6 upload slots ── */}
      <div className="profile-editor-left">
        <div className="upload-grid">
          {Array.from({ length: MAX_SLOTS }, (_, i) => {
            const slot = photos[i];
            const isUploading = uploading[i];
            const isDragTarget = dragOver === i;

            let slotClass = "upload-slot";
            if (isDragTarget) slotClass += " drag-over";
            else if (slot) slotClass += slot.has_face ? " face-ok" : " face-fail";

            return (
              <div
                key={i}
                className={slotClass}
                onDragOver={handleDragOver(i)}
                onDragLeave={handleDragLeave}
                onDrop={handleDrop(i)}
              >
                {isUploading ? (
                  <div className="slot-spinner">
                    <div className="spinner" />
                    <span className="slot-hint">Uploading…</span>
                  </div>
                ) : slot ? (
                  <>
                    <img src={slot.url} alt={`Photo ${i + 1}`} className="slot-photo" />
                    <div className="slot-overlay">
                      <span className={`slot-badge ${slot.has_face ? "ok" : "fail"}`}>
                        {slot.has_face ? `${(slot.score * 100).toFixed(0)}%` : "No face"}
                      </span>
                      <button
                        className="slot-remove"
                        onClick={() => void removePhoto(i)}
                        title="Remove photo"
                        aria-label="Remove photo"
                      >
                        &times;
                      </button>
                    </div>
                  </>
                ) : (
                  <label className="slot-empty">
                    <span className="slot-plus">+</span>
                    <span className="slot-hint">Photo {i + 1}</span>
                    <input
                      type="file"
                      accept="image/*"
                      className="slot-file-input"
                      onChange={handleFileInput(i)}
                    />
                  </label>
                )}
              </div>
            );
          })}
        </div>

        {avgScore !== null && (
          <div className="editor-avg-score">
            Avg confidence: {(avgScore * 100).toFixed(0)}%
            &nbsp;({filledSlots.length}/{MAX_SLOTS} photos)
          </div>
        )}
      </div>

      {/* ── Right panel: composite preview + metadata ── */}
      <div className="profile-editor-right">
        <div className="composite-preview">
          {thumbnail ? (
            <img src={thumbnail} alt="Composite face" className="composite-img" />
          ) : (
            <div className="composite-empty">
              <span>Composite preview</span>
              <span className="composite-hint">Upload photos to generate</span>
            </div>
          )}
        </div>

        <div className="editor-fields">
          <label className="editor-field-label" htmlFor="pe-name">
            Name <span className="required-star">*</span>
          </label>
          <input
            id="pe-name"
            ref={nameRef}
            type="text"
            className="editor-input"
            value={name}
            placeholder="Profile name"
            onChange={(e) => setName(e.target.value)}
          />

          <label className="editor-field-label" htmlFor="pe-desc">
            Description
          </label>
          <textarea
            id="pe-desc"
            className="editor-textarea"
            value={description}
            placeholder="Optional description"
            rows={3}
            onChange={(e) => setDescription(e.target.value)}
          />
        </div>

        {error && <div className="editor-error">{error}</div>}

        <div className="editor-actions">
          <button
            className="btn primary editor-btn"
            onClick={() => void handleSave()}
            disabled={saving}
          >
            {saving ? "Saving…" : "Save"}
          </button>
          {resolvedId && (
            <button
              className="btn danger editor-btn"
              onClick={() => void handleDelete()}
              disabled={saving}
            >
              Delete
            </button>
          )}
          <button
            className="btn secondary editor-btn"
            onClick={onCancel}
            disabled={saving}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
