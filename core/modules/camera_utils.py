"""Platform-aware camera enumeration utility.

Extracted from modules/ui.py to share between the tkinter UI and the FastAPI server.
"""

import platform
import cv2


def get_available_cameras():
    """Returns (list[int], list[str]) -- camera indices and display names.

    Platform behavior:
    - Windows: DirectShow via pygrabber, fallback to OpenCV probe
    - Linux: probes indices 0-9 with cv2.VideoCapture
    - macOS: static [0, 1] to avoid OBSENSOR SIGSEGV
    """
    if platform.system() == "Windows":
        try:
            from pygrabber.dshow_graph import FilterGraph

            graph = FilterGraph()
            devices = graph.get_input_devices()

            camera_indices = list(range(len(devices)))
            camera_names = devices

            # Fallback: if DirectShow finds nothing, try OpenCV
            if not camera_names:
                test_indices = [-1, 0]
                working_cameras = []
                for idx in test_indices:
                    cap = cv2.VideoCapture(idx)
                    if cap.isOpened():
                        working_cameras.append(f"Camera {idx}")
                        cap.release()
                if working_cameras:
                    return test_indices[: len(working_cameras)], working_cameras

            if not camera_names:
                return [], ["No cameras found"]

            return camera_indices, camera_names

        except Exception as e:
            print(f"Error detecting cameras: {e}")
            return [], ["No cameras found"]
    else:
        camera_indices = []
        camera_names = []

        if platform.system() == "Darwin":
            # Static list to avoid OBSENSOR SIGSEGV on macOS
            camera_indices = [0, 1]
            camera_names = ["Camera 0", "Camera 1"]
        else:
            # Linux: probe first 10 indices
            for i in range(10):
                cap = cv2.VideoCapture(i)
                if cap.isOpened():
                    camera_indices.append(i)
                    camera_names.append(f"Camera {i}")
                    cap.release()

        if not camera_names:
            return [], ["No cameras found"]

        return camera_indices, camera_names
