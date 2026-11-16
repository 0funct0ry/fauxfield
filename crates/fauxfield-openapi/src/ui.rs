//! Embedded Swagger UI: vendored gzip assets, the generated page and `init.js` (no inline script).
use flate2::read::GzDecoder;
use std::io::Read;

pub const SWAGGER_UI_VERSION: &str = "5.34.1";

pub const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'";

static BUNDLE_GZ: &[u8] = include_bytes!("../assets/swagger-ui-bundle.js.gz");
static CSS_GZ: &[u8] = include_bytes!("../assets/swagger-ui.css.gz");
static FAVICON_GZ: &[u8] = include_bytes!("../assets/favicon.png.gz");

/// One embedded file, served under `{path}/assets/<name>`.
pub struct Asset {
    pub name: &'static str,
    pub content_type: &'static str,
    /// Gzip bytes when stored compressed (`None` for generated text).
    pub gz: Option<&'static [u8]>,
    pub plain: Option<&'static str>,
}

impl Asset {
    pub fn plain_bytes(&self) -> Vec<u8> {
        if let Some(p) = self.plain {
            return p.as_bytes().to_vec();
        }
        let mut out = Vec::new();
        if let Some(g) = self.gz {
            let _ = GzDecoder::new(g).read_to_end(&mut out);
        }
        out
    }
}

const INIT_JS: &str = "window.onload = function () {\n  window.ui = SwaggerUIBundle({\n    url: './openapi.json',\n    dom_id: '#swagger-ui',\n    deepLinking: true,\n    tryItOutEnabled: false,\n    presets: [SwaggerUIBundle.presets.apis]\n  });\n};\n";

/// Look up an asset by file name.
pub fn asset(name: &str) -> Option<Asset> {
    Some(match name {
        "swagger-ui-bundle.js" => Asset {
            name: "swagger-ui-bundle.js",
            content_type: "text/javascript; charset=utf-8",
            gz: Some(BUNDLE_GZ),
            plain: None,
        },
        "swagger-ui.css" => Asset {
            name: "swagger-ui.css",
            content_type: "text/css; charset=utf-8",
            gz: Some(CSS_GZ),
            plain: None,
        },
        "favicon.png" => Asset {
            name: "favicon.png",
            content_type: "image/png",
            gz: Some(FAVICON_GZ),
            plain: None,
        },
        "init.js" => Asset {
            name: "init.js",
            content_type: "text/javascript; charset=utf-8",
            gz: None,
            plain: Some(INIT_JS),
        },
        _ => return None,
    })
}

/// The page served at `{path}/`. Links are relative so it works behind any prefix or proxy.
pub fn page(title: &str) -> String {
    let t = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let v = SWAGGER_UI_VERSION;
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{t}</title>\n<link rel=\"icon\" type=\"image/png\" href=\"./assets/favicon.png?v={v}\">\n<link rel=\"stylesheet\" href=\"./assets/swagger-ui.css?v={v}\">\n</head>\n<body>\n<div id=\"swagger-ui\"></div>\n<script src=\"./assets/swagger-ui-bundle.js?v={v}\"></script>\n<script src=\"./assets/init.js?v={v}\"></script>\n</body>\n</html>\n"
    )
}

/// Content hash used as the document ETag (FNV-1a 64, hex).
pub fn etag(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("\"{h:016x}\"")
}
