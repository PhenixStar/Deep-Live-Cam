import { useState, useEffect, useCallback } from "react";
import type { Profile } from "../types";
import { ProfileCard } from "./profile-card";

const API_BASE = "http://localhost:8008";

interface ProfileCatalogProps {
  isOpen: boolean;
  onClose: () => void;
  onSelect: (profileId: string) => void;
  onCreateNew: () => void;
}

type FetchState = "idle" | "loading" | "error";

export function ProfileCatalog({
  isOpen,
  onClose,
  onSelect,
  onCreateNew,
}: ProfileCatalogProps) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [fetchState, setFetchState] = useState<FetchState>("idle");

  const loadProfiles = useCallback(async () => {
    setFetchState("loading");
    try {
      const res = await fetch(`${API_BASE}/profiles`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = (await res.json()) as Profile[];
      setProfiles(data);
      setFetchState("idle");
    } catch {
      setFetchState("error");
    }
  }, []);

  useEffect(() => {
    if (isOpen) {
      void loadProfiles();
    }
  }, [isOpen, loadProfiles]);

  // Close on Escape key
  useEffect(() => {
    if (!isOpen) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const handleOverlayClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) onClose();
  };

  const handleSelect = (profileId: string) => {
    onSelect(profileId);
    onClose();
  };

  const handleEdit = (_profileId: string) => {
    // Profile editor phase (Phase 3) — placeholder for now
  };

  return (
    <div className="catalog-overlay" onClick={handleOverlayClick}>
      <div className="catalog-modal" role="dialog" aria-modal="true" aria-label="Face Profiles">
        <div className="catalog-header">
          <h2>Face Profiles</h2>
          <button
            className="catalog-close"
            onClick={onClose}
            aria-label="Close catalog"
          >
            ×
          </button>
        </div>

        <div className="catalog-toolbar">
          <button className="btn primary catalog-create-btn" onClick={onCreateNew}>
            + Create New Profile
          </button>
        </div>

        <div className="catalog-body">
          {fetchState === "loading" && (
            <div className="catalog-status">Loading profiles...</div>
          )}

          {fetchState === "error" && (
            <div className="catalog-status catalog-status-error">
              Failed to load profiles.{" "}
              <button className="catalog-retry" onClick={loadProfiles}>
                Retry
              </button>
            </div>
          )}

          {fetchState === "idle" && profiles.length === 0 && (
            <div className="catalog-status">
              No profiles yet. Create one to get started.
            </div>
          )}

          {fetchState === "idle" && profiles.length > 0 && (
            <div className="catalog-grid">
              {profiles.map((profile) => (
                <ProfileCard
                  key={profile.id}
                  profile={profile}
                  onSelect={handleSelect}
                  onEdit={handleEdit}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
