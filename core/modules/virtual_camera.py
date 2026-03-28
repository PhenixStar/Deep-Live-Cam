"""Virtual camera output for Deep-Live-Cam.

Sends processed frames to a virtual camera device (v4l2loopback on Linux,
OBS Virtual Camera on macOS/Windows) so any video call app sees it as a webcam.

Usage:
    python run.py --virtual-camera --execution-provider cuda
"""

import threading
import numpy as np

_cam = None
_lock = threading.Lock()


def start(width: int = 1280, height: int = 720, fps: int = 30) -> bool:
    """Initialize the virtual camera device. Returns True on success."""
    global _cam
    try:
        import pyvirtualcam
    except ImportError:
        print("[VCAM] pyvirtualcam not installed. Run: pip install pyvirtualcam")
        return False

    with _lock:
        if _cam is not None:
            return True
        try:
            _cam = pyvirtualcam.Camera(width=width, height=height, fps=fps, print_fps=True)
            print(f"[VCAM] Virtual camera started: {_cam.device} ({width}x{height} @ {fps}fps)")
            return True
        except Exception as e:
            print(f"[VCAM] Failed to start virtual camera: {e}")
            _suggest_setup()
            return False


def send(frame: np.ndarray) -> None:
    """Send a BGR frame to the virtual camera (converts to RGB internally)."""
    global _cam
    if _cam is None:
        return
    try:
        # pyvirtualcam expects RGB, OpenCV uses BGR
        import cv2
        rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        # Resize if frame doesn't match camera dimensions
        if rgb.shape[1] != _cam.width or rgb.shape[0] != _cam.height:
            rgb = cv2.resize(rgb, (_cam.width, _cam.height))
        _cam.send(rgb)
        _cam.sleep_until_next_frame()
    except Exception:
        pass  # Don't crash the pipeline on vcam errors


def stop() -> None:
    """Release the virtual camera device."""
    global _cam
    with _lock:
        if _cam is not None:
            try:
                _cam.close()
            except Exception:
                pass
            _cam = None
            print("[VCAM] Virtual camera stopped")


def is_active() -> bool:
    """Check if virtual camera is currently running."""
    return _cam is not None


def _suggest_setup() -> None:
    """Print platform-specific setup instructions."""
    import platform
    system = platform.system()
    if system == "Linux":
        print("[VCAM] Linux setup: sudo modprobe v4l2loopback devices=1 video_nr=10 card_label='Deep-Live-Cam' exclusive_caps=1")
        print("[VCAM] To persist: echo 'v4l2loopback' | sudo tee /etc/modules-load.d/v4l2loopback.conf")
    elif system == "Darwin":
        print("[VCAM] macOS: Install OBS Studio and start its Virtual Camera, or use the built-in DAL plugin.")
    elif system == "Windows":
        print("[VCAM] Windows: Install OBS Studio (includes Virtual Camera driver).")
