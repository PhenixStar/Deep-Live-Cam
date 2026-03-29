import { useState, useEffect, useRef, useCallback } from "react";
import { ControlsPanel } from "./components/controls-panel";
import { VideoCanvas } from "./components/video-canvas";
import { MetricsPanel } from "./components/metrics-panel";
import { ModelManager } from "./components/model-manager";
import { useMetricsWs } from "./hooks/use-metrics-ws";
import { useSystemMetrics } from "./hooks/use-system-metrics";
import { useModels } from "./hooks/use-models";
import { useProfiles } from "./hooks/use-profiles";
import type { Status, Camera, Enhancers, SwapCalibration } from "./types";

const API_BASE = "http://localhost:8008";

export default function App() {
  const [status, setStatus] = useState<Status>("disconnected");
  const [sourceImage, setSourceImage] = useState<string | null>(null);
  const [sourceScore, setSourceScore] = useState<number | null>(null);
  const [fps, setFps] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [cameras, setCameras] = useState<Camera[]>([]);
  const [selectedCamera, setSelectedCamera] = useState<number>(0);
  const [showDebugOverlay, setShowDebugOverlay] = useState(false);
  const [gpuProvider, setGpuProvider] = useState("");
  const [enhancers, setEnhancers] = useState<Enhancers>({
    face_enhancer: false,
    face_enhancer_gpen256: false,
    face_enhancer_gpen512: false,
  });

  const [showModelManager, setShowModelManager] = useState(false);
  const [calibration, setCalibration] = useState<SwapCalibration>({
    swap_offset_x: 0, swap_offset_y: 0, swap_scale: 1.0,
  });

  const [showCatalog, setShowCatalog] = useState(false);

  const wsRef = useRef<WebSocket | null>(null);

  const inferenceMetrics = useMetricsWs(status === "processing");
  const systemMetrics = useSystemMetrics(2000);
  const { models } = useModels();
  const { profiles, activeId: activeProfileId, activate: activateProfile, refresh: refreshProfiles } = useProfiles();

  const faces = inferenceMetrics?.faces ?? [];
  const missingRequired = models.filter((m) => m.required && !m.file_exists);

  // Initial data fetch
  useEffect(() => {
    fetch(`${API_BASE}/cameras`)
      .then((res) => res.json())
      .then((data: { cameras: Camera[] }) => setCameras(data.cameras))
      .catch(() => setError("Failed to load cameras"));

    fetch(`${API_BASE}/settings`)
      .then((res) => res.json())
      .then((data: { fp_ui: Enhancers }) => setEnhancers(data.fp_ui))
      .catch(() => {});

    fetch(`${API_BASE}/health`)
      .then((res) => res.json())
      .then((data: { gpu_provider?: string }) => {
        if (data.gpu_provider) setGpuProvider(data.gpu_provider);
      })
      .catch(() => {});
  }, []);

  const connect = useCallback(() => {
    setStatus("connecting");
    setError(null);
    const ws = new WebSocket("ws://localhost:8008/ws/video");
    wsRef.current = ws;
    ws.binaryType = "arraybuffer";
    ws.addEventListener("open", () => setStatus("connected"));
    ws.addEventListener("message", () => setStatus("processing"));
    ws.addEventListener("error", () => setError("Connection failed — is the backend running?"));
    ws.addEventListener("close", () => {
      setStatus("disconnected");
      wsRef.current = null;
    });
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

  const handleCalibrationChange = useCallback(
    (partial: Partial<SwapCalibration>) => {
      setCalibration((prev) => {
        const next = { ...prev, ...partial };
        // Send to backend
        fetch(`${API_BASE}/settings`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(partial),
        });
        return next;
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
          const data = (await res.json()) as { score?: number };
          setSourceImage(URL.createObjectURL(file));
          setSourceScore(data.score ?? null);
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

  const handleProfileSelect = useCallback(
    async (profileId: string) => {
      await activateProfile(profileId);
      // Sync source image thumbnail from the activated profile
      const profile = profiles.find((p) => p.id === profileId);
      if (profile?.thumbnail_b64) {
        setSourceImage(`data:image/jpeg;base64,${profile.thumbnail_b64}`);
        setSourceScore(profile.score > 0 ? profile.score : null);
      } else {
        setSourceImage(null);
        setSourceScore(null);
      }
    },
    [activateProfile, profiles],
  );

  const activeThumbnail = (() => {
    const active = profiles.find((p) => p.id === activeProfileId);
    if (active?.thumbnail_b64) return `data:image/jpeg;base64,${active.thumbnail_b64}`;
    return sourceImage;
  })();

  // Cleanup WS on unmount
  useEffect(() => {
    return () => {
      wsRef.current?.close();
    };
  }, []);

  // Auto-update check (non-blocking)
  useEffect(() => {
    const timer = setTimeout(async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check();
        if (update) {
          const confirmed = window.confirm(
            `Update ${update.version} available. Download now?`,
          );
          if (confirmed) await update.downloadAndInstall();
        }
      } catch {
        // Non-critical; silently ignore
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

  return (
    <div className="app">
      <header>
        <h1>Deep Forge</h1>
        <div className="header-right">
          <button
            className="btn-models"
            onClick={() => setShowModelManager(true)}
            title="Model Manager"
          >
            Models
            {missingRequired.length > 0 && (
              <span className="models-badge">{missingRequired.length}</span>
            )}
          </button>
          <div className="status">
            <span className="dot" style={{ background: statusColor }} />
            {status} {status === "processing" && `(${fps} fps)`}
          </div>
        </div>
      </header>

      <main>
        <ControlsPanel
          status={status}
          cameras={cameras}
          selectedCamera={selectedCamera}
          enhancers={enhancers}
          sourceImage={sourceImage}
          sourceScore={sourceScore}
          showDebugOverlay={showDebugOverlay}
          onConnect={connect}
          onDisconnect={disconnect}
          onCameraChange={handleCameraChange}
          onEnhancerToggle={handleEnhancerToggle}
          onSourceUpload={handleSourceUpload}
          onToggleDebug={() => setShowDebugOverlay((v) => !v)}
          calibration={calibration}
          onCalibrationChange={handleCalibrationChange}
          profiles={profiles}
          activeProfileId={activeProfileId}
          activeThumbnail={activeThumbnail}
          onProfileSelect={handleProfileSelect}
          onProfileAddNew={() => { refreshProfiles(); setShowCatalog(true); }}
        />
        <VideoCanvas
          wsRef={wsRef}
          status={status}
          onFpsUpdate={setFps}
          faces={faces}
          swapBbox={inferenceMetrics?.swap_bbox ?? null}
          showDebugOverlay={showDebugOverlay}
        />
        <MetricsPanel
          fps={fps}
          inferenceMetrics={inferenceMetrics}
          systemMetrics={systemMetrics}
          gpuProvider={gpuProvider}
          sourceScore={sourceScore}
        />
      </main>

      {missingRequired.length > 0 && (
        <div className="models-warning">
          {missingRequired.length} required model
          {missingRequired.length > 1 ? "s" : ""} missing — face swap
          unavailable.{" "}
          <button
            className="models-warning-link"
            onClick={() => setShowModelManager(true)}
          >
            Download now
          </button>
        </div>
      )}

      {error && <div className="error">{error}</div>}

      {showModelManager && (
        <ModelManager onClose={() => setShowModelManager(false)} />
      )}
    </div>
  );
}
