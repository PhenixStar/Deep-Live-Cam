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
}

export interface SystemMetrics {
  cpu_percent: number;
  ram_used_gb: number;
  ram_total_gb: number;
}

export interface ModelInfo {
  name: string;
  file: string;
  file_exists: boolean;
  size_mb: number | null;
  required: boolean;
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
