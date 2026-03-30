import type { ModelInfo } from "../types";
import { useModels, hasDownloadUrl, type ReloadResult } from "../hooks/use-models";

interface ModelManagerProps {
  onClose: () => void;
}

function statusLabel(model: ModelInfo, downloadPct: number | undefined): string {
  if (downloadPct !== undefined) return "Downloading...";
  if (model.file_exists) return "Downloaded";
  return "Missing";
}

function statusClass(model: ModelInfo, downloadPct: number | undefined): string {
  if (downloadPct !== undefined) return "model-status downloading";
  if (model.file_exists) return "model-status downloaded";
  return "model-status missing";
}

function reloadResultLabel(result: ReloadResult): string {
  const loaded = Object.values(result).filter((v) => v === "loaded").length;
  const total = Object.keys(result).length;
  return `${loaded}/${total} models loaded`;
}

export function ModelManager({ onClose }: ModelManagerProps) {
  const { models, downloading, reloading, reloadResult, downloadModel, reloadModels, refresh } = useModels();

  const missingRequired = models.filter((m) => m.required && !m.file_exists);

  return (
    <div className="mm-overlay" onClick={onClose}>
      <div className="mm-panel" onClick={(e) => e.stopPropagation()}>
        <div className="mm-header">
          <h2>Model Manager</h2>
          <button className="mm-close" onClick={onClose}>
            ✕
          </button>
        </div>

        {missingRequired.length > 0 && (
          <div className="mm-warning">
            {missingRequired.length} required model
            {missingRequired.length > 1 ? "s" : ""} missing — face swap
            unavailable until downloaded.
          </div>
        )}

        <div className="mm-list">
          {models.map((model) => {
            const pct = downloading[model.name];
            const hasUrl = hasDownloadUrl(model);

            return (
              <div key={model.file} className="mm-card">
                <div className="mm-card-top">
                  <div className="mm-card-name">
                    {model.name}
                    {model.required && (
                      <span className="mm-badge required">Required</span>
                    )}
                  </div>
                  <span className={statusClass(model, pct)}>
                    {statusLabel(model, pct)}
                  </span>
                </div>

                {model.file_exists && model.size_mb !== null && (
                  <div className="mm-size">
                    {model.size_mb.toFixed(0)} MB
                  </div>
                )}

                {pct !== undefined && (
                  <div className="mm-progress-bar">
                    <div
                      className="mm-progress-fill"
                      style={{ width: `${pct}%` }}
                    />
                    <span className="mm-progress-label">{pct}%</span>
                  </div>
                )}

                {!model.file_exists && pct === undefined && hasUrl && (
                  <button
                    className="btn primary mm-dl-btn"
                    onClick={() => downloadModel(model)}
                  >
                    Download
                  </button>
                )}

                {!model.file_exists && !hasUrl && (
                  <div className="mm-manual-note">
                    Place <code>{model.file}</code> in models dir manually.
                  </div>
                )}
              </div>
            );
          })}
        </div>

        <div className="mm-footer">
          <button className="btn primary" onClick={refresh}>
            Refresh
          </button>
          <button
            className="btn primary"
            onClick={reloadModels}
            disabled={reloading}
          >
            {reloading ? "Reloading..." : "Reload Models"}
          </button>
          {reloadResult && (
            <span className="mm-reload-result">
              {reloadResultLabel(reloadResult)}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
