import { useState, useEffect, useRef, type ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Status, Camera, Enhancers, Resolution, SwapCalibration, Profile, InputMode } from "../types";
import { SourceSelector } from "./source-selector";

const API_BASE = "http://localhost:8008";

const RESOLUTIONS: Resolution[] = [
  { width: 640,  height: 480,  label: "480p (640x480)"   },
  { width: 1280, height: 720,  label: "720p (1280x720)"  },
  { width: 1920, height: 1080, label: "1080p (1920x1080)" },
];

interface ServerModeInfo {
  remote_mode: boolean;
  bind_address: string;
  api_token?: string;
}

interface ControlsPanelProps {
  status: Status;
  cameras: Camera[];
  selectedCamera: number;
  enhancers: Enhancers;
  sourceImage: string | null;
  sourceScore: number | null;
  showDebugOverlay: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
  onCameraChange: (e: ChangeEvent<HTMLSelectElement>) => void;
  onEnhancerToggle: (key: keyof Enhancers, checked: boolean) => void;
  onSourceUpload: (e: ChangeEvent<HTMLInputElement>) => void;
  onToggleDebug: () => void;
  calibration: SwapCalibration;
  onCalibrationChange: (cal: Partial<SwapCalibration>) => void;
  // Profile-based source selector
  profiles: Profile[];
  activeProfileId: string | null;
  activeThumbnail: string | null;
  onProfileSelect: (profileId: string) => void;
  onProfileAddNew: () => void;
}

const ENHANCER_LABELS: { key: keyof Enhancers; label: string }[] = [
  { key: "face_enhancer",      label: "GFPGAN"   },
  { key: "face_enhancer_gpen256", label: "GPEN-256" },
  { key: "face_enhancer_gpen512", label: "GPEN-512" },
];

export function ControlsPanel({
  status,
  cameras: initialCameras,
  selectedCamera,
  enhancers,
  sourceImage,
  sourceScore,
  showDebugOverlay,
  onConnect,
  onDisconnect,
  onCameraChange,
  onEnhancerToggle,
  onSourceUpload,
  onToggleDebug,
  calibration,
  onCalibrationChange,
  profiles,
  activeProfileId,
  activeThumbnail,
  onProfileSelect,
  onProfileAddNew,
}: ControlsPanelProps) {
  const [cameras, setCameras] = useState<Camera[]>(initialCameras);
  const [refreshing, setRefreshing] = useState(false);
  const [resolution, setResolution] = useState<Resolution>(RESOLUTIONS[0]);
  const [serverMode, setServerMode] = useState<ServerModeInfo | null>(null);
  const [tokenCopied, setTokenCopied] = useState(false);
  const [inputMode, setInputMode] = useState<InputMode>("camera");
  const [videoFilename, setVideoFilename] = useState<string | null>(null);
  const [videoUploading, setVideoUploading] = useState(false);
  const videoFileRef = useRef<HTMLInputElement>(null);

  // Camera status polling
  const [cameraReady, setCameraReady] = useState(false);
  const cameraPollingRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Server mode toggle (restart sidecar)
  const [restarting, setRestarting] = useState(false);

  // Recording state
  const [recording, setRecording] = useState(false);
  const [recordingSeconds, setRecordingSeconds] = useState(0);
  const recordingTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Sync if parent updates cameras (initial load)
  useEffect(() => {
    if (initialCameras.length > 0) {
      setCameras(initialCameras);
    }
  }, [initialCameras]);

  // Poll /camera/status every 2s until camera is ready
  useEffect(() => {
    if (cameraReady) return;
    const poll = () => {
      fetch(`${API_BASE}/camera/status`)
        .then((res) => res.json())
        .then((data: { available?: boolean }) => {
          if (data.available) {
            setCameraReady(true);
            if (cameraPollingRef.current) {
              clearInterval(cameraPollingRef.current);
              cameraPollingRef.current = null;
            }
          }
        })
        .catch(() => {});
    };
    poll(); // immediate first check
    cameraPollingRef.current = setInterval(poll, 2000);
    return () => {
      if (cameraPollingRef.current) clearInterval(cameraPollingRef.current);
    };
  }, [cameraReady]);

  // Recording timer
  useEffect(() => {
    if (recording) {
      setRecordingSeconds(0);
      recordingTimerRef.current = setInterval(() => {
        setRecordingSeconds((s) => s + 1);
      }, 1000);
    } else {
      if (recordingTimerRef.current) {
        clearInterval(recordingTimerRef.current);
        recordingTimerRef.current = null;
      }
    }
    return () => {
      if (recordingTimerRef.current) clearInterval(recordingTimerRef.current);
    };
  }, [recording]);

  // Fetch server mode info from /health on mount
  useEffect(() => {
    fetch(`${API_BASE}/health`)
      .then((res) => res.json())
      .then((data: { remote_mode?: boolean; bind_address?: string }) => {
        if (data.remote_mode !== undefined) {
          setServerMode({
            remote_mode: data.remote_mode,
            bind_address: data.bind_address ?? "127.0.0.1:8008",
          });
        }
      })
      .catch(() => {});
  }, []);

  const handleRefreshCameras = async () => {
    setRefreshing(true);
    try {
      const res = await fetch(`${API_BASE}/cameras/refresh`, { method: "POST" });
      if (res.ok) {
        const data = (await res.json()) as { cameras: Camera[] };
        setCameras(data.cameras);
      }
    } catch {
      // Silently ignore — cameras list stays as-is
    } finally {
      setRefreshing(false);
    }
  };

  const handleResolutionChange = async (e: ChangeEvent<HTMLSelectElement>) => {
    const selected = RESOLUTIONS.find((r) => r.label === e.target.value);
    if (!selected) return;
    setResolution(selected);
    try {
      await fetch(`${API_BASE}/settings`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          resolution_width:  selected.width,
          resolution_height: selected.height,
        }),
      });
    } catch {
      // Best-effort; local state already updated
    }
  };

  const handleCopyToken = async () => {
    if (!serverMode?.api_token) return;
    try {
      await navigator.clipboard.writeText(serverMode.api_token);
      setTokenCopied(true);
      setTimeout(() => setTokenCopied(false), 2000);
    } catch {
      // Clipboard not available
    }
  };

  const handleSwitchToCamera = async () => {
    try {
      await fetch(`${API_BASE}/input/camera`, { method: "POST" });
    } catch {
      // Best-effort
    }
    setInputMode("camera");
    setVideoFilename(null);
  };

  const formatRecordingTime = (seconds: number) => {
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  };

  const handleToggleRemote = async (enable: boolean) => {
    setRestarting(true);
    try {
      await invoke("restart_sidecar", { remote: enable });
      // Refresh server mode info after restart
      setTimeout(() => {
        fetch(`${API_BASE}/health`)
          .then((res) => res.json())
          .then((data: { remote_mode?: boolean; bind_address?: string }) => {
            if (data.remote_mode !== undefined) {
              setServerMode({
                remote_mode: data.remote_mode,
                bind_address: data.bind_address ?? "127.0.0.1:8008",
              });
            }
          })
          .catch(() => {})
          .finally(() => setRestarting(false));
      }, 3000);
    } catch {
      setRestarting(false);
    }
  };

  const handleRecordingToggle = async () => {
    if (recording) {
      try {
        await fetch(`${API_BASE}/recording/stop`, { method: "POST" });
      } catch {
        // Best-effort
      }
      setRecording(false);
    } else {
      try {
        const res = await fetch(`${API_BASE}/recording/start`, { method: "POST" });
        if (res.ok) {
          setRecording(true);
        }
      } catch {
        // Best-effort
      }
    }
  };

  const handleVideoFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setVideoUploading(true);
    try {
      const form = new FormData();
      form.append("file", file);
      const res = await fetch(`${API_BASE}/input/video`, { method: "POST", body: form });
      if (res.ok) {
        setInputMode("video_file");
        setVideoFilename(file.name);
      }
    } catch {
      // Upload failed — stay in camera mode
    } finally {
      setVideoUploading(false);
      // Reset so the same file can be re-selected
      if (videoFileRef.current) videoFileRef.current.value = "";
    }
  };

  return (
    <section className="controls">
      <SourceSelector
        profiles={profiles}
        activeProfileId={activeProfileId}
        onSelect={onProfileSelect}
        onAddNew={onProfileAddNew}
        thumbnail={activeThumbnail}
      />
      {/* Fallback file upload — hidden, used by profile editor */}
      <input
        id="source-upload-fallback"
        type="file"
        accept="image/*"
        onChange={onSourceUpload}
        style={{ display: "none" }}
      />
      {sourceScore !== null && (
        <div className="source-score">
          Detection: {(sourceScore * 100).toFixed(0)}%
        </div>
      )}

      <div className="input-source">
        <label>Input Source</label>
        <div className="input-source-toggle">
          <button
            className={`btn-toggle ${inputMode === "camera" ? "active" : ""}`}
            onClick={handleSwitchToCamera}
          >
            Camera
          </button>
          <button
            className={`btn-toggle ${inputMode === "video_file" ? "active" : ""}`}
            onClick={() => videoFileRef.current?.click()}
            disabled={videoUploading}
          >
            {videoUploading ? "Uploading..." : "Video File"}
          </button>
        </div>
        <input
          ref={videoFileRef}
          type="file"
          accept=".mp4,.avi,.webm,.mov"
          onChange={handleVideoFileChange}
          style={{ display: "none" }}
        />
        {inputMode === "video_file" && videoFilename && (
          <div className="video-filename" title={videoFilename}>
            {videoFilename}
          </div>
        )}
      </div>

      <div className="camera-select">
        <div className="camera-select-header">
          <label>
            Camera
            {cameraReady ? (
              <span className="camera-status-badge ready">Ready</span>
            ) : (
              <span className="camera-status-badge opening">Opening...</span>
            )}
          </label>
          <button
            className="btn-refresh"
            onClick={handleRefreshCameras}
            disabled={refreshing}
            title="Refresh camera list"
          >
            {refreshing ? "..." : "Refresh"}
          </button>
        </div>
        <select value={selectedCamera} onChange={onCameraChange} disabled={inputMode === "video_file"}>
          {cameras.map((c) => (
            <option key={c.index} value={c.index}>
              {c.name}
            </option>
          ))}
        </select>
      </div>

      <div className="resolution-select">
        <label>Resolution</label>
        <select value={resolution.label} onChange={handleResolutionChange}>
          {RESOLUTIONS.map((r) => (
            <option key={r.label} value={r.label}>
              {r.label}
            </option>
          ))}
        </select>
      </div>

      <div className="enhancers">
        <label>Face Enhancers</label>
        {ENHANCER_LABELS.map(({ key, label }) => (
          <label key={key} className="toggle">
            <input
              type="checkbox"
              checked={enhancers[key]}
              onChange={(e) => onEnhancerToggle(key, e.target.checked)}
            />
            {label}
          </label>
        ))}
      </div>

      <div className="debug-toggle-row">
        <label className="toggle">
          <input
            type="checkbox"
            className="debug-toggle"
            checked={showDebugOverlay}
            onChange={onToggleDebug}
          />
          Debug Overlay
        </label>
      </div>

      <div className="calibration">
        <label className="section-label">Swap Calibration</label>
        <div className="cal-row">
          <span className="cal-label">X Offset</span>
          <input type="range" min={-50} max={50} step={1}
            value={calibration.swap_offset_x}
            onChange={e => onCalibrationChange({ swap_offset_x: Number(e.target.value) })} />
          <span className="cal-value">{calibration.swap_offset_x}</span>
        </div>
        <div className="cal-row">
          <span className="cal-label">Y Offset</span>
          <input type="range" min={-50} max={50} step={1}
            value={calibration.swap_offset_y}
            onChange={e => onCalibrationChange({ swap_offset_y: Number(e.target.value) })} />
          <span className="cal-value">{calibration.swap_offset_y}</span>
        </div>
        <div className="cal-row">
          <span className="cal-label">Scale</span>
          <input type="range" min={0.5} max={2.0} step={0.05}
            value={calibration.swap_scale}
            onChange={e => onCalibrationChange({ swap_scale: Number(e.target.value) })} />
          <span className="cal-value">{calibration.swap_scale.toFixed(2)}</span>
        </div>
        <button className="btn-reset" onClick={() => onCalibrationChange({ swap_offset_x: 0, swap_offset_y: 0, swap_scale: 1.0 })}>
          Reset
        </button>
      </div>

      <div className="actions">
        {status === "disconnected" ? (
          <button className="btn primary" onClick={onConnect}>
            Start Live
          </button>
        ) : (
          <button className="btn danger" onClick={onDisconnect}>
            Stop
          </button>
        )}
      </div>

      <div className="recording-section">
        <label>Recording</label>
        <button
          className={`btn-record ${recording ? "active" : ""}`}
          onClick={handleRecordingToggle}
        >
          {recording ? (
            <>
              <span className="record-dot active" />
              {`Recording... (${formatRecordingTime(recordingSeconds)})`}
            </>
          ) : (
            <>
              <span className="record-dot" />
              Record
            </>
          )}
        </button>
      </div>

      {serverMode && (
        <div className="server-mode">
          <label>Server Mode</label>
          {restarting ? (
            <div className="server-mode-info">
              <span className="server-mode-badge restarting">Restarting...</span>
            </div>
          ) : serverMode.remote_mode ? (
            <div className="server-mode-info">
              <div className="server-mode-row">
                <span className="server-mode-badge remote">Remote</span>
                <button
                  className="btn-mode-toggle"
                  onClick={() => handleToggleRemote(false)}
                  title="Switch to local mode"
                >
                  Disable
                </button>
              </div>
              <div className="server-mode-row">
                <span className="server-mode-label">Bind</span>
                <span className="server-mode-value">{serverMode.bind_address}</span>
              </div>
              {serverMode.api_token && (
                <div className="server-mode-row">
                  <span className="server-mode-label">Token</span>
                  <span className="server-mode-value token">
                    {serverMode.api_token}
                  </span>
                  <button className="btn-copy" onClick={handleCopyToken}>
                    {tokenCopied ? "Copied" : "Copy"}
                  </button>
                </div>
              )}
            </div>
          ) : (
            <div className="server-mode-info">
              <div className="server-mode-row">
                <span className="server-mode-badge local">Local only</span>
                <button
                  className="btn-mode-toggle"
                  onClick={() => handleToggleRemote(true)}
                  title="Restart with --remote flag for LAN access"
                >
                  Enable Remote
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
