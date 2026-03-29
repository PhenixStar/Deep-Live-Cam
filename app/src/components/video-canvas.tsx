import { useEffect, useRef, useCallback } from "react";
import type { Status, FaceRect } from "../types";

interface VideoCanvasProps {
  wsRef: React.RefObject<WebSocket | null>;
  status: Status;
  onFpsUpdate: (fps: number) => void;
  faces: FaceRect[];
  showDebugOverlay: boolean;
}

export function VideoCanvas({
  wsRef,
  status,
  onFpsUpdate,
  faces,
  showDebugOverlay,
}: VideoCanvasProps) {
  const videoRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);

  // Draw face bounding boxes on the debug overlay canvas
  const drawOverlay = useCallback(
    (width: number, height: number) => {
      const canvas = overlayRef.current;
      if (!canvas) return;
      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, width, height);

      if (!showDebugOverlay || faces.length === 0) return;

      ctx.strokeStyle = "#22c55e";
      ctx.lineWidth = 2;
      ctx.font = "12px monospace";
      ctx.fillStyle = "#22c55e";

      for (const face of faces) {
        ctx.strokeRect(face.x, face.y, face.w, face.h);
        const label = `${(face.score * 100).toFixed(0)}%`;
        ctx.fillText(label, face.x + 2, face.y - 4 > 0 ? face.y - 4 : face.y + 14);
      }
    },
    [faces, showDebugOverlay],
  );

  // Attach WS message handler to render JPEG frames
  useEffect(() => {
    const ws = wsRef.current;
    if (!ws) return;

    let frameCount = 0;
    let lastTime = performance.now();

    const handleMessage = async (event: MessageEvent) => {
      const blob = new Blob([event.data as BlobPart], { type: "image/jpeg" });
      try {
        const bitmap = await createImageBitmap(blob);
        const canvas = videoRef.current;
        if (canvas) {
          canvas.width = bitmap.width;
          canvas.height = bitmap.height;
          const ctx = canvas.getContext("2d");
          ctx?.drawImage(bitmap, 0, 0);
          drawOverlay(bitmap.width, bitmap.height);
        }
        bitmap.close();
      } catch {
        // Corrupt frame — skip silently
      }

      frameCount++;
      const now = performance.now();
      if (now - lastTime >= 1000) {
        onFpsUpdate(Math.round(frameCount / ((now - lastTime) / 1000)));
        frameCount = 0;
        lastTime = now;
      }
    };

    ws.addEventListener("message", handleMessage);
    return () => ws.removeEventListener("message", handleMessage);
  }, [wsRef, onFpsUpdate, drawOverlay]);

  // Redraw overlay whenever faces or toggle changes without a new frame
  useEffect(() => {
    const canvas = videoRef.current;
    if (canvas && canvas.width > 0) {
      drawOverlay(canvas.width, canvas.height);
    }
  }, [drawOverlay]);

  return (
    <section className="preview">
      <div className="canvas-stack">
        <canvas ref={videoRef} className="video-canvas" />
        <canvas ref={overlayRef} className="debug-canvas" />
      </div>
      {status === "disconnected" && (
        <div className="overlay">Click &quot;Start Live&quot; to begin face swap</div>
      )}
    </section>
  );
}
