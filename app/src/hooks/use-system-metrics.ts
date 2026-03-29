import { useState, useEffect } from "react";
import type { SystemMetrics } from "../types";

export function useSystemMetrics(intervalMs = 2000): SystemMetrics | null {
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function poll() {
      try {
        // Dynamic import so the module resolves at runtime only inside Tauri
        const { invoke } = await import("@tauri-apps/api/core");
        const data = await invoke<SystemMetrics>("get_system_metrics");
        if (!cancelled) setMetrics(data);
      } catch {
        // Outside Tauri or command not yet registered — stay null
      }
    }

    poll();
    const id = window.setInterval(poll, intervalMs);

    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [intervalMs]);

  return metrics;
}
