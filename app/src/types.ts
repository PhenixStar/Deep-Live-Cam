export type Status = "disconnected" | "connecting" | "connected" | "processing";

export interface Camera {
  index: number;
  name: string;
}

export interface Enhancers {
  face_enhancer: boolean;
  face_enhancer_gpen256: boolean;
  face_enhancer_gpen512: boolean;
}

export interface FaceRect {
  x: number;
  y: number;
  w: number;
  h: number;
  score: number;
}

export interface FrameMetrics {
  detect_ms: number;
  swap_ms: number;
  total_ms: number;
  face_count: number;
  faces: FaceRect[];
  swap_bbox: FaceRect | null;
}

export interface SwapCalibration {
  swap_offset_x: number;
  swap_offset_y: number;
  swap_scale: number;
  detection_interval?: number;
}

export interface SystemMetrics {
  cpu_percent: number;
  ram_used_gb: number;
  ram_total_gb: number;
}

export interface ModelInfo {
  name: string;
  path: string;
  // url_suffix matches the JSON key sent by the backend; the backend's custom
  // serializer emits a full download URL despite the "suffix" name.
  url_suffix: string;
  fallback_url: string;
  file_exists: boolean;
  file_size_mb: number;
  size_mb: number;
  required: boolean;
  loaded?: boolean;
  description?: string;
}

export interface Resolution {
  width: number;
  height: number;
  label: string;
}

export interface Profile {
  id: string;
  name: string;
  description: string;
  photo_count: number;
  score: number;
  thumbnail_b64: string | null;
}

export type InputMode = "camera" | "video_file";

export interface InputStatus {
  input_mode: InputMode;
  filename?: string;
}

export interface ProviderInfo {
  name: string;
  available: boolean;
  description: string;
}

export interface ProvidersResponse {
  active: string;
  available: ProviderInfo[];
}
