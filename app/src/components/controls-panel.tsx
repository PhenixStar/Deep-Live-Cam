import type { ChangeEvent } from "react";
import type { Status, Camera, Enhancers } from "../types";

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
}

const ENHANCER_LABELS: { key: keyof Enhancers; label: string }[] = [
  { key: "face_enhancer", label: "GFPGAN" },
  { key: "face_enhancer_gpen256", label: "GPEN-256" },
  { key: "face_enhancer_gpen512", label: "GPEN-512" },
];

export function ControlsPanel({
  status,
  cameras,
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
}: ControlsPanelProps) {
  return (
    <section className="controls">
      <div className="source-face">
        <label>Source Face</label>
        {sourceImage ? (
          <img src={sourceImage} alt="source" className="face-preview" />
        ) : (
          <div className="placeholder">No face selected</div>
        )}
        {sourceScore !== null && (
          <div className="source-score">
            Detection: {(sourceScore * 100).toFixed(0)}%
          </div>
        )}
        <input type="file" accept="image/*" onChange={onSourceUpload} />
      </div>

      <div className="camera-select">
        <label>Camera</label>
        <select value={selectedCamera} onChange={onCameraChange}>
          {cameras.map((c) => (
            <option key={c.index} value={c.index}>
              {c.name}
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
    </section>
  );
}
