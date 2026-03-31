import { useState, useEffect, useCallback, useRef } from "react";
import type { ModelInfo } from "../types";

const API_BASE = "http://localhost:8008";

// Detect if running inside Tauri webview (vs plain browser).
const IS_TAURI = Boolean(
  typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__
);

/// Check if a model has a download URL (from the backend manifest).
export function hasDownloadUrl(model: ModelInfo): boolean {
  return Boolean(model.url_suffix || model.fallback_url);
}

/// Get the best download URL for a model.
function getDownloadUrl(model: ModelInfo): string | null {
  return model.url_suffix || model.fallback_url || null;
}

interface DownloadProgressEvent {
  name: string;
  downloaded: number;
  total: number;
}

export type ReloadResult = Record<string, string>;

export function useModels(): {
  models: ModelInfo[];
  downloading: Record<string, number>;
  reloading: boolean;
  reloadResult: ReloadResult | null;
  downloadModel: (model: ModelInfo) => void;
  reloadModels: () => Promise<void>;
  refresh: () => void;
} {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [downloading, setDownloading] = useState<Record<string, number>>({});
  const [reloading, setReloading] = useState(false);
  const [reloadResult, setReloadResult] = useState<ReloadResult | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const fetchModels = useCallback(() => {
    fetch(`${API_BASE}/models/status`)
      .then((r) => r.json())
      .then((data: { models: ModelInfo[] }) => setModels(data.models))
      .catch(() => {});
  }, []);

  useEffect(() => {
    fetchModels();

    let active = true;

    if (IS_TAURI) {
      import("@tauri-apps/api/event").then(({ listen }) => {
        if (!active) return;
        listen<DownloadProgressEvent>("model_download_progress", (event) => {
          if (!active) return;
          const { name, downloaded, total } = event.payload;
          const pct = total > 0 ? Math.round((downloaded / total) * 100) : 0;
          setDownloading((prev) => ({ ...prev, [name]: pct }));
        }).then((fn) => {
          unlistenRef.current = fn;
        });
      });
    }

    return () => {
      active = false;
      unlistenRef.current?.();
    };
  }, [fetchModels]);

  const downloadModel = useCallback(
    async (model: ModelInfo) => {
      const url = getDownloadUrl(model);
      if (!url) return;

      const modelPath = model.path;
      if (!modelPath) return;

      setDownloading((prev) => ({ ...prev, [model.name]: 0 }));

      try {
        if (IS_TAURI) {
          // Tauri path: download via sidecar with progress events
          const { invoke } = await import("@tauri-apps/api/core");
          const modelsDir = await invoke<string>("get_models_dir");
          const dest = modelsDir + "/" + modelPath;
          await invoke<void>("download_model", { name: model.name, url, dest });
        } else {
          // Browser mode: no Tauri IPC, can't write to disk.
          // Open the model URL in a new tab so user can download manually.
          window.open(url, "_blank");
          throw new Error("Download models via the desktop app or place files in the models directory manually.");
        }

        setDownloading((prev) => {
          const next = { ...prev };
          delete next[model.name];
          return next;
        });
        fetchModels();
        // Auto-reload models into the server after a successful download.
        fetch(`${API_BASE}/models/reload`, { method: "POST" }).catch(() => {});
      } catch {
        setDownloading((prev) => {
          const next = { ...prev };
          delete next[model.name];
          return next;
        });
      }
    },
    [fetchModels],
  );

  const reloadModels = useCallback(async () => {
    setReloading(true);
    setReloadResult(null);
    try {
      const res = await fetch(`${API_BASE}/models/reload`, { method: "POST" });
      const data: { status: string; models: ReloadResult } = await res.json();
      setReloadResult(data.models);
      fetchModels();
    } catch {
      setReloadResult({ error: "Request failed" });
    } finally {
      setReloading(false);
    }
  }, [fetchModels]);

  return { models, downloading, reloading, reloadResult, downloadModel, reloadModels, refresh: fetchModels };
}
