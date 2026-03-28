"""FastAPI backend for Deep-Live-Cam Tauri app.

Exposes face-swap pipeline via HTTP + WebSocket for real-time video streaming.
Designed to run as a Tauri sidecar process on localhost:8008.
"""

import asyncio
import sys
import os
import threading

import cv2
import numpy as np
from fastapi import FastAPI, WebSocket, UploadFile, File, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
import uvicorn

# Add project root to path so modules/ is importable
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import modules.globals as globals
from modules.face_analyser import get_one_face
from modules.processors.frame.core import get_frame_processors_modules
from modules.camera_utils import get_available_cameras

app = FastAPI(title="Deep-Live-Cam Server", version="0.1.0")
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

# Shared state
_source_face = None
_source_face_lock = threading.Lock()
_active_camera_index: int = 0
_camera_lock = threading.Lock()


def _init_providers():
    """Initialize ONNX execution providers."""
    import onnxruntime as ort
    providers = ort.get_available_providers()
    if "CUDAExecutionProvider" in providers:
        globals.execution_providers = ["CUDAExecutionProvider", "CPUExecutionProvider"]
    else:
        globals.execution_providers = ["CPUExecutionProvider"]
    globals.frame_processors = ["face_swapper"]


@app.on_event("startup")
async def startup():
    _init_providers()
    print(f"[SERVER] Providers: {globals.execution_providers}")


@app.get("/health")
async def health():
    return {"status": "ok", "providers": globals.execution_providers}


@app.post("/source")
async def upload_source(file: UploadFile = File(...)):
    """Upload source face image."""
    global _source_face
    contents = await file.read()
    nparr = np.frombuffer(contents, np.uint8)
    img = cv2.imdecode(nparr, cv2.IMREAD_COLOR)
    if img is None:
        return JSONResponse(status_code=400, content={"error": "Invalid image"})

    face = get_one_face(img)
    if face is None:
        return JSONResponse(status_code=400, content={"error": "No face detected in source"})

    with _source_face_lock:
        _source_face = face
    return {"status": "ok", "message": "Source face loaded"}


@app.get("/cameras")
async def list_cameras():
    """List available cameras with index and display name."""
    indices, names = get_available_cameras()
    return {
        "cameras": [
            {"index": i, "name": n}
            for i, n in zip(indices, names)
        ]
    }


@app.post("/camera/{index}")
async def set_camera(index: int):
    """Switch the active camera. Takes effect on next WS connection."""
    global _active_camera_index
    indices, _ = get_available_cameras()
    if index not in indices:
        return JSONResponse(status_code=400, content={"error": f"Camera {index} not available"})
    with _camera_lock:
        _active_camera_index = index
    return {"status": "ok", "camera_index": index}


@app.get("/settings")
async def get_settings():
    """Read current face processor settings."""
    return {"fp_ui": globals.fp_ui, "frame_processors": globals.frame_processors}


@app.post("/settings")
async def update_settings(settings: dict):
    """Toggle face enhancers on/off."""
    valid_keys = {"face_enhancer", "face_enhancer_gpen256", "face_enhancer_gpen512"}
    for key, value in settings.items():
        if key in valid_keys and isinstance(value, bool):
            globals.fp_ui[key] = value
    return {"status": "ok", "fp_ui": globals.fp_ui}


@app.websocket("/ws/video")
async def video_stream(ws: WebSocket):
    """WebSocket endpoint: captures webcam, applies face swap, streams JPEG frames."""
    await ws.accept()

    with _camera_lock:
        cam_idx = _active_camera_index
    cap = cv2.VideoCapture(cam_idx)
    if not cap.isOpened():
        await ws.send_json({"error": f"Cannot open camera {cam_idx}"})
        await ws.close()
        return

    cap.set(cv2.CAP_PROP_FRAME_WIDTH, 1280)
    cap.set(cv2.CAP_PROP_FRAME_HEIGHT, 720)

    try:
        while True:
            ret, frame = cap.read()
            if not ret:
                await asyncio.sleep(0.01)
                continue

            # Re-fetch processors each frame so fp_ui toggle changes apply immediately
            frame_processors = get_frame_processors_modules(globals.frame_processors)

            with _source_face_lock:
                source = _source_face

            # Run all active processors (swapper + enhancers) via generic interface
            if source is not None:
                for processor in frame_processors:
                    frame = processor.process_frame(source, frame)

            # Encode as JPEG and send
            _, buffer = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, 80])
            await ws.send_bytes(buffer.tobytes())

            # Yield to event loop (~30fps target)
            await asyncio.sleep(0.033)

    except WebSocketDisconnect:
        pass
    finally:
        cap.release()


if __name__ == "__main__":
    uvicorn.run(app, host="127.0.0.1", port=8008, log_level="info")
