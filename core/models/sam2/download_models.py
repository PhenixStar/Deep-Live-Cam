"""
Download SAM2 ONNX models from HuggingFace.

Usage:
    python models/sam2/download_models.py [variant]

Variants:
    tiny        - SAM2 Hiera-Tiny (smallest, fastest) ~155 MB
    small       - SAM2 Hiera-Small (good balance) ~183 MB
    base_plus   - SAM2 Hiera-Base+ (higher accuracy) ~360 MB
    large       - SAM2 Hiera-Large (most accurate) ~910 MB

Default: tiny (recommended for real-time LivePortrait use)

Models are downloaded from:
    https://huggingface.co/vietanhdev/segment-anything-2-onnx-models
"""

import os
import sys
import urllib.request
import zipfile

HUGGINGFACE_BASE_URL = "https://huggingface.co/vietanhdev/segment-anything-2-onnx-models/resolve/main"

VARIANTS = {
    "tiny": "sam2_hiera_tiny",
    "small": "sam2_hiera_small",
    "base_plus": "sam2_hiera_base_plus",
    "large": "sam2_hiera_large",
}

def download_variant(variant_key: str = "tiny", output_dir: str = None):
    """Download and extract a SAM2 ONNX model variant."""
    if output_dir is None:
        output_dir = os.path.dirname(os.path.abspath(__file__))

    if variant_key not in VARIANTS:
        print(f"Unknown variant: {variant_key}")
        print(f"Available variants: {', '.join(VARIANTS.keys())}")
        return False

    variant_name = VARIANTS[variant_key]
    zip_filename = f"{variant_name}.zip"
    zip_url = f"{HUGGINGFACE_BASE_URL}/{zip_filename}"
    zip_path = os.path.join(output_dir, zip_filename)

    encoder_path = os.path.join(output_dir, f"{variant_name}.encoder.onnx")
    decoder_path = os.path.join(output_dir, f"{variant_name}.decoder.onnx")

    # Check if already downloaded
    if os.path.exists(encoder_path) and os.path.exists(decoder_path):
        print(f"[SAM2] Models already exist for variant '{variant_key}':")
        print(f"  Encoder: {encoder_path}")
        print(f"  Decoder: {decoder_path}")
        return True

    print(f"[SAM2] Downloading {variant_key} variant from HuggingFace...")
    print(f"  URL: {zip_url}")
    print(f"  Destination: {output_dir}")

    try:
        # Download zip
        def progress_hook(count, block_size, total_size):
            percent = int(count * block_size * 100 / total_size) if total_size > 0 else 0
            percent = min(percent, 100)
            sys.stdout.write(f"\r  Downloading: {percent}%")
            sys.stdout.flush()

        urllib.request.urlretrieve(zip_url, zip_path, reporthook=progress_hook)
        print()  # newline after progress

        # Extract
        print(f"  Extracting...")
        with zipfile.ZipFile(zip_path, 'r') as z:
            z.extractall(output_dir)

        # The zip extracts to a subdirectory; move files up if needed
        extracted_dir = os.path.join(output_dir, variant_name)
        if os.path.isdir(extracted_dir):
            for fname in os.listdir(extracted_dir):
                src = os.path.join(extracted_dir, fname)
                dst = os.path.join(output_dir, f"{variant_name}.{fname}")
                if not os.path.exists(dst):
                    os.rename(src, dst)
            # Clean up extracted directory
            try:
                os.rmdir(extracted_dir)
            except OSError:
                pass

        # Clean up zip
        if os.path.exists(zip_path):
            os.remove(zip_path)

        # Verify
        if os.path.exists(encoder_path) and os.path.exists(decoder_path):
            print(f"[SAM2] Successfully downloaded {variant_key} variant!")
            print(f"  Encoder: {encoder_path}")
            print(f"  Decoder: {decoder_path}")
            return True
        else:
            # Try alternate naming (files might be named without the variant prefix)
            alt_encoder = os.path.join(output_dir, "encoder.onnx")
            alt_decoder = os.path.join(output_dir, "decoder.onnx")
            if os.path.exists(alt_encoder) and not os.path.exists(encoder_path):
                os.rename(alt_encoder, encoder_path)
            if os.path.exists(alt_decoder) and not os.path.exists(decoder_path):
                os.rename(alt_decoder, decoder_path)

            if os.path.exists(encoder_path) and os.path.exists(decoder_path):
                print(f"[SAM2] Successfully downloaded {variant_key} variant!")
                return True
            else:
                print(f"[SAM2] Warning: Expected files not found after extraction.")
                print(f"  Looking for: {encoder_path}")
                print(f"  Looking for: {decoder_path}")
                print(f"  Contents of {output_dir}:")
                for f in os.listdir(output_dir):
                    print(f"    {f}")
                return False

    except Exception as e:
        print(f"\n[SAM2] Download failed: {e}")
        # Clean up partial download
        if os.path.exists(zip_path):
            os.remove(zip_path)
        return False


if __name__ == "__main__":
    variant = sys.argv[1] if len(sys.argv) > 1 else "tiny"
    success = download_variant(variant)
    if not success:
        sys.exit(1)
