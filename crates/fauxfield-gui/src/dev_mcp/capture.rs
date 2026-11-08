//! Window capture for `ui.screenshot` (SPEC §11.4). A `Capture` trait with tiers: offscreen
//! render (unavailable: gpui 0.2.2 cannot render to an image, SPEC §20.5), macOS `screencapture`,
//! Linux `grim` then ImageMagick `import`. Free of GPUI so it runs off the UI thread.

use std::process::Output;

use serde::{Deserialize, Serialize};

/// Window bounds in screen points (global coordinates) and the backing scale factor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl WindowBounds {
    /// Expected image size in pixels: bounds times scale.
    pub fn pixels(&self) -> (u32, u32) {
        (
            (self.width * self.scale).round() as u32,
            (self.height * self.scale).round() as u32,
        )
    }
}

/// Below this size a capture of a full window cannot be real (macOS hands back an empty or
/// wallpaper-only image without Screen Recording permission).
const MIN_REAL_BYTES: usize = 8 * 1024;
/// A real window has at least this many distinct colours; a placeholder or blank has fewer.
const MIN_DISTINCT_COLOURS: usize = 8;

pub fn permission_hint() -> &'static str {
    "grant Screen Recording to the app running Fauxfield (your terminal or Claude) in System \
     Settings > Privacy & Security > Screen & System Audio Recording, then restart it"
}

/// Runs an external program; injectable so tier logic is testable without a display.
pub type Runner = dyn Fn(&str, &[String]) -> std::io::Result<Output> + Send + Sync;

fn real_runner(prog: &str, args: &[String]) -> std::io::Result<Output> {
    std::process::Command::new(prog).args(args).output()
}

/// One way to turn window bounds into PNG bytes.
pub trait Capture: Send + Sync {
    /// Reported in every screenshot result.
    fn tier(&self) -> &'static str;
    /// PNG bytes of the screen region `bounds`.
    fn grab(&self, bounds: WindowBounds) -> Result<Vec<u8>, String>;
}

fn tmp_png() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "fauxfield-shot-{}-{}.png",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Runs `prog args` (with `{out}` replaced by a temp path) and reads the PNG it wrote.
fn run_to_png(
    runner: &Runner,
    prog: &str,
    args: Vec<String>,
    hint: &str,
) -> Result<Vec<u8>, String> {
    let path = tmp_png();
    let args: Vec<String> = args
        .into_iter()
        .map(|a| a.replace("{out}", &path.to_string_lossy()))
        .collect();
    let out = runner(prog, &args).map_err(|e| format!("could not run {prog}: {e}. {hint}"))?;
    let result = if out.status.success() {
        std::fs::read(&path).map_err(|_| format!("{prog} wrote no image. {hint}"))
    } else {
        Err(format!(
            "{prog} failed ({}): {}. {hint}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    };
    let _ = std::fs::remove_file(&path);
    result
}

fn region(b: &WindowBounds) -> [i64; 4] {
    [
        b.x.round() as i64,
        b.y.round() as i64,
        b.width.round() as i64,
        b.height.round() as i64,
    ]
}

pub struct Screencapture {
    runner: Box<Runner>,
}

impl Screencapture {
    pub fn new() -> Self {
        Self {
            runner: Box::new(real_runner),
        }
    }
    pub fn with_runner(runner: Box<Runner>) -> Self {
        Self { runner }
    }
}

impl Default for Screencapture {
    fn default() -> Self {
        Self::new()
    }
}

impl Capture for Screencapture {
    fn tier(&self) -> &'static str {
        "screencapture"
    }
    fn grab(&self, b: WindowBounds) -> Result<Vec<u8>, String> {
        let [x, y, w, h] = region(&b);
        run_to_png(
            &*self.runner,
            "screencapture",
            vec![
                "-x".into(),
                "-R".into(),
                format!("{x},{y},{w},{h}"),
                "{out}".into(),
            ],
            &format!("Likely fix: {}", permission_hint()),
        )
    }
}

/// Wayland `grim`, with ImageMagick `import` (X11, xvfb in CI) as the fallback.
pub struct Linux {
    runner: Box<Runner>,
}

impl Linux {
    pub fn new() -> Self {
        Self {
            runner: Box::new(real_runner),
        }
    }
    pub fn with_runner(runner: Box<Runner>) -> Self {
        Self { runner }
    }
}

impl Default for Linux {
    fn default() -> Self {
        Self::new()
    }
}

impl Capture for Linux {
    fn tier(&self) -> &'static str {
        "linux"
    }
    fn grab(&self, b: WindowBounds) -> Result<Vec<u8>, String> {
        let [x, y, w, h] = region(&b);
        let grim = run_to_png(
            &*self.runner,
            "grim",
            vec!["-g".into(), format!("{x},{y} {w}x{h}"), "{out}".into()],
            "",
        );
        if let Ok(png) = grim {
            return Ok(png);
        }
        run_to_png(
            &*self.runner,
            "import",
            vec![
                "-window".into(),
                "root".into(),
                "-crop".into(),
                format!("{w}x{h}+{x}+{y}"),
                "+repage".into(),
                "{out}".into(),
            ],
            "Install `grim` (Wayland) or ImageMagick `import` (X11, e.g. under xvfb) to capture \
             screenshots",
        )
    }
}

/// Picks the tier for this platform. Offscreen render is not available with gpui 0.2.2.
pub fn select() -> Result<Box<dyn Capture>, String> {
    if cfg!(target_os = "macos") {
        Ok(Box::new(Screencapture::new()))
    } else if cfg!(target_os = "linux") {
        Ok(Box::new(Linux::new()))
    } else {
        Err("screenshots are not supported on this platform yet (macOS and Linux only)".into())
    }
}

/// Decoded image: 8-bit RGBA.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| format!("not a PNG: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("bad PNG: {e}"))?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

pub fn encode(img: &Image) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&img.rgba))
        .map_err(|e| format!("could not encode PNG: {e}"))?;
    Ok(out)
}

/// Checks a capture before it is returned: expected size (bounds times scale, +-2 px), not a
/// tiny file, not flat. Failures name the likely permission.
pub fn verify(bytes: &[u8], bounds: &WindowBounds) -> Result<Image, String> {
    let img = decode(bytes)?;
    let (ew, eh) = bounds.pixels();
    if img.width.abs_diff(ew) > 2 || img.height.abs_diff(eh) > 2 {
        return Err(format!(
            "captured {}x{} px but the window is {ew}x{eh} px; the window may be off-screen or \
             on another display",
            img.width, img.height
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for px in img.rgba.chunks_exact(4).step_by(7) {
        seen.insert([px[0], px[1], px[2]]);
        if seen.len() >= MIN_DISTINCT_COLOURS {
            break;
        }
    }
    if bytes.len() < MIN_REAL_BYTES || seen.len() < MIN_DISTINCT_COLOURS {
        return Err(format!(
            "the capture is blank or flat ({} bytes, {} colours), which is what the OS returns \
             without permission: {}",
            bytes.len(),
            seen.len(),
            permission_hint()
        ));
    }
    Ok(img)
}

/// Crops `rect` (x, y, w, h in window points) from a full-window image.
pub fn crop(img: &Image, rect: [f32; 4], scale: f32) -> Result<Image, String> {
    let x0 = (rect[0] * scale).floor().max(0.0) as u32;
    let y0 = (rect[1] * scale).floor().max(0.0) as u32;
    let x1 = (((rect[0] + rect[2]) * scale).ceil() as u32).min(img.width);
    let y1 = (((rect[1] + rect[3]) * scale).ceil() as u32).min(img.height);
    if x1 <= x0 || y1 <= y0 {
        return Err("the element is outside the window".into());
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        let start = ((y * img.width + x0) * 4) as usize;
        rgba.extend_from_slice(&img.rgba[start..start + (w * 4) as usize]);
    }
    Ok(Image {
        width: w,
        height: h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    fn bounds() -> WindowBounds {
        WindowBounds {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
            scale: 2.0,
        }
    }

    fn gradient(w: u32, h: u32) -> Image {
        // Pseudo-random noise: incompressible, like a real window full of text.
        let mut x = 0x9E37_79B9u32;
        let rgba = (0..w * h)
            .flat_map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                [x as u8, (x >> 8) as u8, (x >> 16) as u8, 255]
            })
            .collect();
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn verify_accepts_a_real_image_of_the_right_size() {
        let png = encode(&gradient(200, 160)).unwrap();
        // Noise-like gradient compresses poorly enough to pass the size floor.
        assert!(
            verify(&png, &bounds()).is_ok(),
            "{:?}",
            verify(&png, &bounds()).err()
        );
    }

    #[test]
    fn verify_rejects_wrong_size() {
        let png = encode(&gradient(210, 160)).unwrap();
        let e = verify(&png, &bounds()).err().unwrap();
        assert!(e.contains("captured 210x160"), "{e}");
    }

    #[test]
    fn verify_rejects_flat_image_with_permission_hint() {
        let flat = Image {
            width: 200,
            height: 160,
            rgba: vec![30; 200 * 160 * 4],
        };
        let e = verify(&encode(&flat).unwrap(), &bounds()).err().unwrap();
        assert!(e.contains("Screen Recording"), "{e}");
    }

    #[test]
    fn verify_rejects_non_png() {
        assert!(verify(b"nope", &bounds()).is_err());
    }

    #[test]
    fn crop_uses_scale() {
        let img = gradient(80, 60);
        let c = crop(&img, [5.0, 5.0, 10.0, 8.0], 2.0).unwrap();
        assert_eq!((c.width, c.height), (20, 16));
        let first = ((10 * 80 + 10) * 4) as usize;
        assert_eq!(c.rgba[..4], img.rgba[first..first + 4]);
        assert!(crop(&img, [100.0, 0.0, 5.0, 5.0], 2.0).is_err());
    }

    #[test]
    fn linux_falls_back_to_import_and_names_the_tools() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let c = calls.clone();
        let runner: Box<Runner> = Box::new(move |prog, _| {
            c.lock().unwrap().push(prog.to_owned());
            Err(std::io::Error::other("missing"))
        });
        let e = Linux::with_runner(runner).grab(bounds()).err().unwrap();
        assert_eq!(*calls.lock().unwrap(), ["grim", "import"]);
        assert!(e.contains("grim") && e.contains("import"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn screencapture_failure_carries_the_permission_hint() {
        let runner: Box<Runner> = Box::new(|_, _| {
            Ok(Output {
                status: std::process::ExitStatus::from_raw(256),
                stdout: vec![],
                stderr: b"could not create image".to_vec(),
            })
        });
        let e = Screencapture::with_runner(runner)
            .grab(bounds())
            .err()
            .unwrap();
        assert!(e.contains("Screen Recording"), "{e}");
    }

    #[test]
    fn screencapture_passes_the_rounded_region() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let s = seen.clone();
        let runner: Box<Runner> = Box::new(move |_, args| {
            *s.lock().unwrap() = args.join(" ");
            Err(std::io::Error::other("stop"))
        });
        let _ = Screencapture::with_runner(runner).grab(bounds());
        assert!(seen.lock().unwrap().starts_with("-x -R 10,20,100,80 "));
    }
}
