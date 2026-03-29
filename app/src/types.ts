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
}

export interface SystemMetrics {
  cpu_percent: number;
  ram_used_gb: number;
  ram_total_gb: number;
}
