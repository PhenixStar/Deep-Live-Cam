#!/bin/bash
cd "$(dirname "$0")"
source .venv/bin/activate
export CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-0}
export DEEP_LIVE_CAM_MODELS_DIR="${DEEP_LIVE_CAM_MODELS_DIR:-$(pwd)/models}"
python run.py --execution-provider cuda "$@"
