import { useState, useEffect, useCallback } from "react";
import type { Profile } from "../types";

const API_BASE = "http://localhost:8008";

interface UseProfilesResult {
  profiles: Profile[];
  activeId: string | null;
  activate: (id: string) => Promise<void>;
  refresh: () => void;
}

export function useProfiles(): UseProfilesResult {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);

  const refresh = useCallback(() => {
    fetch(`${API_BASE}/profiles`)
      .then((r) => r.json())
      .then((data: Profile[]) => {
        setProfiles(Array.isArray(data) ? data : []);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const activate = useCallback(async (id: string) => {
    try {
      const res = await fetch(`${API_BASE}/profiles/${id}/activate`, {
        method: "POST",
      });
      if (res.ok) {
        setActiveId(id);
      }
    } catch {
      // Best-effort; caller can refresh if needed
    }
  }, []);

  return { profiles, activeId, activate, refresh };
}
