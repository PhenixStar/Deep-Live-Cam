import { useState, useEffect, useRef, useCallback } from "react";

const API_BASE = "http://localhost:8008";

type Status = "disconnected" | "connecting" | "connected" | "processing";

interface Camera {
  index: number;
  name: string;
}

interface Enhancers {
  face_enhancer: boolean;
  face_enhancer_gpen256: boolean;
  face_enhancer_gpen512: boolean;
}

export default function App() {
  const [status, setStatus] = useState<Status>("disconnected");
  const [sourceImage, setSourceImage] = useState<string | null>(null);
  const [fps, setFps] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [cameras, setCameras] = useState<Camera[]>([]);
  const [selectedCamera, setSelectedCamera] = useState<number>(0);
  const [enhancers, setEnhancers] = useState<Enhancers>({
    face_enhancer: false,
    face_enhancer_gpen256: false,
    face_enhancer_gpen512: false,
  });
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // Fetch available cameras and current settings on mount
  useEffect(() => {
    fetch(`${API_BASE}/cameras`)
      .then((res) => res.json())
      .then((data) => setCameras(data.cameras))
      .catch(() => setError("Failed to load cameras"));

    fetch(`${API_BASE}/settings`)
      .then((res) => res.json())
      .then((data) => setEnhancers(data.fp_ui))
      .catch(() => {});
  }, []);

  const connect = useCallback(() => {
    setStatus("connecting");
    setError(null);
    const ws = new WebSocket("ws://localhost:8008/ws/video");
    wsRef.current = ws;

    ws.binaryType = "arraybuffer";
    let frameCount = 0;
    let lastTime = performance.now();

    ws.onopen = () => setStatus("connected");

    ws.onmessage = (event) => {
      setStatus("processing");
      const blob = new Blob([event.data], { type: "image/jpeg" });
      const url = URL.createObjectURL(blob);
      const img = new Image();
      img.onload = () => {
        const canvas = canvasRef.current;
        if (canvas) {
          canvas.width = img.width;
          canvas.height = img.height;
          const ctx = canvas.getContext("2d");
          ctx?.drawImage(img, 0, 0);
        }
        URL.revokeObjectURL(url);
        frameCount++;
        const now = performance.now();
        if (now - lastTime >= 1000) {
          setFps(Math.round(frameCount / ((now - lastTime) / 1000)));
          frameCount = 0;
          lastTime = now;
        }
      };
      img.src = url;
    };

    ws.onerror = () => setError("Connection failed — is the backend running?");
    ws.onclose = () => {
      setStatus("disconnected");
      wsRef.current = null;
    };
  }, []);

  const disconnect = useCallback(() => {
    wsRef.current?.close();
    setStatus("disconnected");
  }, []);

  const handleCameraChange = useCallback(
    async (e: React.ChangeEvent<HTMLSelectElement>) => {
      const idx = Number(e.target.value);
      try {
        const res = await fetch(`${API_BASE}/camera/${idx}`, { method: "POST" });
        if (res.ok) {
          setSelectedCamera(idx);
          // Reconnect WS to pick up new camera
          if (wsRef.current) {
            disconnect();
            setTimeout(connect, 300);
          }
        } else {
          setError("Failed to switch camera");
        }
      } catch {
        setError("Backend not reachable");
      }
    },
    [connect, disconnect],
  );

  const handleEnhancerToggle = useCallback(
    async (key: keyof Enhancers, checked: boolean) => {
      setEnhancers((prev) => ({ ...prev, [key]: checked }));
      await fetch(`${API_BASE}/settings`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ [key]: checked }),
      });
    },
    [],
  );

  const handleSourceUpload = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      const formData = new FormData();
      formData.append("file", file);
      try {
        const res = await fetch(`${API_BASE}/source`, {
          method: "POST",
          body: formData,
        });
        if (res.ok) {
          setSourceImage(URL.createObjectURL(file));
          setError(null);
        } else {
          setError("Failed to upload source face");
        }
      } catch {
        setError("Backend not reachable");
      }
    },
    [],
  );

  useEffect(() => {
    return () => {
      wsRef.current?.close();
    };
  }, []);

  // Check for app updates after a short delay (don't block startup)
  useEffect(() => {
    const timer = setTimeout(async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check();
        if (update) {
          const confirmed = window.confirm(
            `Update ${update.version} available. Download now?`,
          );
          if (confirmed) {
            await update.downloadAndInstall();
          }
        }
      } catch {
        // Update check is non-critical; silently ignore failures
      }
    }, 5000);
    return () => clearTimeout(timer);
  }, []);

  const statusColor = {
    disconnected: "#666",
    connecting: "#f59e0b",
    connected: "#22c55e",
    processing: "#3b82f6",
  }[status];

  const enhancerLabels: { key: keyof Enhancers; label: string }[] = [
    { key: "face_enhancer", label: "GFPGAN" },
    { key: "face_enhancer_gpen256", label: "GPEN-256" },
    { key: "face_enhancer_gpen512", label: "GPEN-512" },
  ];

  return (
    <div className="app">
      <header>
        <h1>Deep Live Cam</h1>
        <div className="status">
          <span className="dot" style={{ background: statusColor }} />
          {status} {status === "processing" && `(${fps} fps)`}
        </div>
      </header>

      <main>
        <section className="controls">
          <div className="source-face">
            <label>Source Face</label>
            {sourceImage ? (
              <img src={sourceImage} alt="source" className="face-preview" />
            ) : (
              <div className="placeholder">No face selected</div>
            )}
            <input
              type="file"
              accept="image/*"
              onChange={handleSourceUpload}
            />
          </div>

          <div className="camera-select">
            <label>Camera</label>
            <select value={selectedCamera} onChange={handleCameraChange}>
              {cameras.map((c) => (
                <option key={c.index} value={c.index}>
                  {c.name}
                </option>
              ))}
            </select>
          </div>

          <div className="enhancers">
            <label>Face Enhancers</label>
            {enhancerLabels.map(({ key, label }) => (
              <label key={key} className="toggle">
                <input
                  type="checkbox"
                  checked={enhancers[key]}
                  onChange={(e) => handleEnhancerToggle(key, e.target.checked)}
                />
                {label}
              </label>
            ))}
          </div>

          <div className="actions">
            {status === "disconnected" ? (
              <button className="btn primary" onClick={connect}>
                Start Live
              </button>
            ) : (
              <button className="btn danger" onClick={disconnect}>
                Stop
              </button>
            )}
          </div>
        </section>

        <section className="preview">
          <canvas ref={canvasRef} className="video-canvas" />
          {status === "disconnected" && (
            <div className="overlay">
              Click &quot;Start Live&quot; to begin face swap
            </div>
          )}
        </section>
      </main>

      {error && <div className="error">{error}</div>}
    </div>
  );
}
