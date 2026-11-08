//! Capture prototype (de-risks M1.5): window bounds in screen coordinates plus a `screencapture`
//! probe on macOS. Replaced by the `Capture` trait and `ui.screenshot` in M1.5. Free of GPUI so
//! it can run off the UI thread.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Window bounds in screen points (global coordinates) and the backing scale factor.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

/// A capture smaller than this cannot be a real window: macOS returns a wallpaper-only or empty
/// image when Screen Recording is denied. Heuristic: the PNG decoder is not a dependency.
const MIN_REAL_BYTES: u64 = 8 * 1024;

pub fn permission_hint() -> &'static str {
    "grant Screen Recording to the app running Fauxfield (your terminal or Claude) in System \
     Settings > Privacy & Security > Screen & System Audio Recording, then restart it"
}

#[cfg(target_os = "macos")]
pub fn probe(b: WindowBounds) -> Result<Value, String> {
    use std::process::Command;

    let path = std::env::temp_dir().join(format!("fauxfield-probe-{}.png", std::process::id()));
    let region = format!(
        "{},{},{},{}",
        b.x.round(),
        b.y.round(),
        b.width.round(),
        b.height.round()
    );
    let out = Command::new("screencapture")
        .args(["-x", "-R", &region])
        .arg(&path)
        .output()
        .map_err(|e| format!("could not run screencapture: {e}"))?;
    let result = (|| {
        if !out.status.success() {
            return Err(format!(
                "screencapture failed ({}): {}. Likely fix: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim(),
                permission_hint()
            ));
        }
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let (w, h) = png_size(&path).ok_or("screencapture wrote no readable PNG")?;
        let (ew, eh) = (
            (b.width * b.scale).round() as i64,
            (b.height * b.scale).round() as i64,
        );
        if (i64::from(w) - ew).abs() > 2 || (i64::from(h) - eh).abs() > 2 {
            return Err(format!(
                "captured {w}x{h} px but the window is {ew}x{eh} px; the window may be off-screen or on another display"
            ));
        }
        if bytes < MIN_REAL_BYTES {
            return Err(format!(
                "the capture is blank ({bytes} bytes), which is what macOS returns without permission: {}",
                permission_hint()
            ));
        }
        Ok(serde_json::json!({
            "tier": "screencapture",
            "bounds": b,
            "region": region,
            "png": { "width": w, "height": h, "bytes": bytes },
        }))
    })();
    let _ = std::fs::remove_file(&path);
    result
}

#[cfg(not(target_os = "macos"))]
pub fn probe(_: WindowBounds) -> Result<Value, String> {
    Err(
        "the capture probe is implemented on macOS only; Linux (grim, import) arrives in M1.5"
            .into(),
    )
}

/// Width and height from the PNG IHDR chunk.
#[cfg(target_os = "macos")]
fn png_size(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    if &head[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
    Some((w, h))
}
