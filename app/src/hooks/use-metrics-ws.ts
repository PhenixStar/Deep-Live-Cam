import { useState, useEffect, useRef } from "react";
import type { FrameMetrics } from "../types";

const METRICS_WS_URL = "ws://localhost:8008/ws/metrics";

export function useMetricsWs(enabled: boolean): FrameMetrics | null {
  const [metrics, setMetrics] = useState<FrameMetrics | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!enabled) {
      wsRef.current?.close();
      wsRef.current = null;
      setMetrics(null);
      return;
    }

    const ws = new WebSocket(METRICS_WS_URL);
    wsRef.current = ws;

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data as string) as FrameMetrics;
        setMetrics(data);
      } catch {
        // Malformed JSON — skip frame
      }
    };

    ws.onerror = () => {
      // Metrics WS is non-critical; silently ignore
    };

    ws.onclose = () => {
      wsRef.current = null;
    };

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, [enabled]);

  return metrics;
}
