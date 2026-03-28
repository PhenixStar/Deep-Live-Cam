//! Integration tests for dlc-server.
//!
//! Drive the Axum router in-process via `tower::ServiceExt::oneshot`.
//! No TCP socket is opened; tests are fast and self-contained.
//!
//! Run:
//!   cargo test -p dlc-server
//!
//! Include the ignored live-server smoke test:
//!   cargo test -p dlc-server -- --include-ignored

use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt; // Router::oneshot

use dlc_server::router::build_router;
use dlc_server::test_state;

// ---------------------------------------------------------------------------
// Helper: drain response body into a serde_json::Value
// ---------------------------------------------------------------------------

async fn json_body(body: Body) -> Value {
    let bytes = body
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not valid JSON")
}

// ---------------------------------------------------------------------------
// Helpers: build a fresh router for each test (avoids shared-state races)
// ---------------------------------------------------------------------------

fn app() -> axum::Router {
    build_router(test_state())
}

// ---------------------------------------------------------------------------
// GET /health
// ---------------------------------------------------------------------------

/// Returns HTTP 200 with {"status": "ok", "backend": "rust"}.
#[tokio::test]
async fn health_returns_ok() {
    let resp = app()
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = json_body(resp.into_body()).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["backend"], "rust");
}

// ---------------------------------------------------------------------------
// GET /cameras
// ---------------------------------------------------------------------------

/// Returns HTTP 200 and a JSON object with a "cameras" array.
#[tokio::test]
async fn cameras_returns_json_array() {
    let resp = app()
        .oneshot(Request::builder().uri("/cameras").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = json_body(resp.into_body()).await;
    assert!(body["cameras"].is_array(), "body must contain a 'cameras' array");
}

/// Stub implementation always returns at least one entry with index + name.
#[tokio::test]
async fn cameras_has_index_and_name_fields() {
    let resp = app()
        .oneshot(Request::builder().uri("/cameras").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = json_body(resp.into_body()).await;
    let cams = body["cameras"].as_array().unwrap();
    assert!(!cams.is_empty(), "stub must return at least one camera");

    let cam = &cams[0];
    assert!(cam["index"].is_number(), "camera.index must be a number");
    assert!(cam["name"].is_string(),  "camera.name must be a string");
}

// ---------------------------------------------------------------------------
// GET /settings
// ---------------------------------------------------------------------------

/// Returns HTTP 200 with fp_ui containing the three boolean toggles.
#[tokio::test]
async fn get_settings_has_fp_ui_booleans() {
    let resp = app()
        .oneshot(Request::builder().uri("/settings").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = json_body(resp.into_body()).await;
    assert!(body["fp_ui"].is_object(), "body must contain fp_ui object");

    for key in &["face_enhancer", "face_enhancer_gpen256", "face_enhancer_gpen512"] {
        assert!(
            body["fp_ui"][key].is_boolean(),
            "fp_ui.{key} must be a boolean"
        );
    }
}

// ---------------------------------------------------------------------------
// POST /settings
// ---------------------------------------------------------------------------

/// {"face_enhancer": true} → HTTP 200, {"status": "ok"}.
#[tokio::test]
async fn post_settings_returns_ok() {
    let payload = serde_json::json!({"face_enhancer": true}).to_string();

    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/settings")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp.into_body()).await;
    assert_eq!(body["status"], "ok");
}

// ---------------------------------------------------------------------------
// POST /source
// ---------------------------------------------------------------------------

/// Valid JPEG → HTTP 200, {"status": "ok", "bytes": N}.
///
/// Skipped (not failed) when test_assets/source.jpg is absent so CI without
/// assets still passes cleanly.
#[tokio::test]
async fn source_upload_valid_jpeg_returns_ok() {
    // CARGO_MANIFEST_DIR = dlc-server/
    let source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()          // rust-engine/
        .unwrap()
        .parent()          // core/
        .unwrap()
        .join("test_assets/source.jpg");

    if !source_path.exists() {
        eprintln!("SKIP source_upload_valid_jpeg_returns_ok — {source_path:?} not found");
        return;
    }

    let image_bytes = std::fs::read(&source_path).unwrap();
    let body_bytes = multipart_body("file", "source.jpg", &image_bytes);

    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/source")
                .method("POST")
                .header("content-type", multipart_content_type())
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp.into_body()).await;
    assert_eq!(body["status"], "ok");
    assert!(body["bytes"].as_u64().unwrap_or(0) > 0);
}

/// Garbage bytes → HTTP 422 Unprocessable Entity.
#[tokio::test]
async fn source_upload_invalid_image_returns_422() {
    let body_bytes = multipart_body("file", "bad.jpg", b"not-a-valid-image-xyzxyz");

    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/source")
                .method("POST")
                .header("content-type", multipart_content_type())
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Missing multipart body altogether → HTTP 400 Bad Request.
#[tokio::test]
async fn source_upload_no_field_returns_400() {
    // Empty multipart (boundary present, no parts).
    let empty_body = format!("--{BOUNDARY}--\r\n");

    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/source")
                .method("POST")
                .header("content-type", multipart_content_type())
                .body(Body::from(empty_body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// POST /camera/{index}
// ---------------------------------------------------------------------------

/// Camera index 0 is always available in the stub → HTTP 200.
#[tokio::test]
async fn set_camera_valid_index_returns_ok() {
    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/camera/0")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp.into_body()).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["camera_index"], 0);
}

/// Camera index 99 is not enumerated → HTTP 400.
#[tokio::test]
async fn set_camera_unknown_index_returns_400() {
    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/camera/99")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// POST /swap/image  (models absent → 503)
// ---------------------------------------------------------------------------

/// Without loaded ONNX models the endpoint must return 503, not panic.
#[tokio::test]
async fn swap_image_without_models_returns_503() {
    let source_img = minimal_jpeg();
    let body_bytes = two_field_multipart(&source_img, &source_img);

    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");

    let resp = app()
        .oneshot(
            Request::builder()
                .uri("/swap/image")
                .method("POST")
                .header("content-type", &content_type)
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();

    // Without models the server must return 503; it must not panic or 500.
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// ---------------------------------------------------------------------------
// Multipart helpers
// ---------------------------------------------------------------------------

const BOUNDARY: &str = "dlctestboundary1234";

fn multipart_content_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

/// Build a single-field multipart/form-data body.
fn multipart_body(field_name: &str, filename: &str, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    let header = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\nContent-Type: image/jpeg\r\n\r\n"
    );
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(data);
    buf.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    buf
}

/// Build a two-field multipart body with "source" and "target".
fn two_field_multipart(source: &[u8], target: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();

    for (name, data) in [("source", source), ("target", target)] {
        let header = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{name}.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
        );
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(data);
        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    buf
}

/// A minimal valid 1×1 white JPEG (known-good bytes, no external file needed).
fn minimal_jpeg() -> Vec<u8> {
    // 1×1 white JPEG produced by: convert -size 1x1 xc:white minimal.jpg | xxd
    vec![
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01,
        0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xff, 0xdb, 0x00, 0x43,
        0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09,
        0x09, 0x08, 0x0a, 0x0c, 0x14, 0x0d, 0x0c, 0x0b, 0x0b, 0x0c, 0x19, 0x12,
        0x13, 0x0f, 0x14, 0x1d, 0x1a, 0x1f, 0x1e, 0x1d, 0x1a, 0x1c, 0x1c, 0x20,
        0x24, 0x2e, 0x27, 0x20, 0x22, 0x2c, 0x23, 0x1c, 0x1c, 0x28, 0x37, 0x29,
        0x2c, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1f, 0x27, 0x39, 0x3d, 0x38, 0x32,
        0x3c, 0x2e, 0x33, 0x34, 0x32, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x01,
        0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xff, 0xc4, 0x00, 0x1f, 0x00, 0x00,
        0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0xff, 0xc4, 0x00, 0xb5, 0x10, 0x00, 0x02, 0x01, 0x03,
        0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7d,
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
        0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08,
        0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72,
        0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
        0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
        0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75,
        0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
        0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3,
        0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6,
        0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9,
        0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
        0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4,
        0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xff, 0xda, 0x00, 0x08, 0x01, 0x01,
        0x00, 0x00, 0x3f, 0x00, 0xfb, 0xd7, 0xff, 0xd9,
    ]
}
