use std::fs::File;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use cosmic::iced::Color;
use cosmic::widget::icon;
use image::{DynamicImage, ImageReader, Limits, RgbaImage};
use url::Url;

use crate::album_color::dominant_album_color;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: u32 = 5;
/// Covers are downscaled to this size; the popup never shows them larger.
const MAX_DIMENSION: u32 = 256;
const MAX_DECODED_DIMENSION: u32 = 8192;
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;
const REDACTED_LENGTH: usize = 80;
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// A decoded cover, ready to be drawn. The handle is created once so the
/// renderer can cache it.
#[derive(Clone, Debug)]
pub struct LoadedArt {
    pub handle: icon::Handle,
    pub color: Option<Color>,
}

struct LoadOptions {
    timeout: Duration,
    max_bytes: u64,
    max_dimension: u32,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            timeout: TIMEOUT,
            max_bytes: MAX_BYTES,
            max_dimension: MAX_DIMENSION,
        }
    }
}

/// Loads and decodes the cover behind an MPRIS `mpris:artUrl`.
///
/// Supports `file:`, `http(s):` and base64 `data:` URLs. This blocks, so it
/// must not run on the UI thread.
pub fn load(url: &str) -> Result<LoadedArt, String> {
    load_with(url, &LoadOptions::default())
}

fn load_with(url: &str, options: &LoadOptions) -> Result<LoadedArt, String> {
    let bytes = fetch(url, options)?;
    decode(bytes, options.max_dimension)
}

#[must_use]
pub fn is_file_url(url: &str) -> bool {
    scheme(url).as_deref() == Some("file")
}

/// Shortens a URL for log messages, so inline `data:` covers are not dumped.
#[must_use]
pub fn redact(url: &str) -> String {
    if url.chars().count() <= REDACTED_LENGTH {
        url.to_owned()
    } else {
        let prefix = url.chars().take(REDACTED_LENGTH).collect::<String>();
        format!("{prefix}…")
    }
}

fn scheme(url: &str) -> Option<String> {
    url.split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
}

fn fetch(url: &str, options: &LoadOptions) -> Result<Vec<u8>, String> {
    match scheme(url).as_deref() {
        Some("file") => read_file(url, options.max_bytes),
        Some("http" | "https") => download(url, options),
        Some("data") => decode_data_url(url, options.max_bytes),
        _ => Err("unsupported URL scheme".to_owned()),
    }
}

fn read_file(url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let path = file_url_to_path(url).ok_or_else(|| "invalid file URL".to_owned())?;
    let file = File::open(&path).map_err(|error| format!("unable to open file: {error}"))?;
    let length = file
        .metadata()
        .map_err(|error| format!("unable to read file: {error}"))?
        .len();
    if length > max_bytes {
        return Err("album art is too large".to_owned());
    }

    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("unable to read file: {error}"))?;
    ensure_within_limit(&bytes, max_bytes)?;
    Ok(bytes)
}

fn download(url: &str, options: &LoadOptions) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(options.timeout))
        .max_redirects(MAX_REDIRECTS)
        .user_agent(USER_AGENT)
        .build()
        .into();

    agent
        .get(url)
        .call()
        .map_err(|error| format!("download failed: {error}"))?
        .body_mut()
        .with_config()
        .limit(options.max_bytes)
        .read_to_vec()
        .map_err(|error| format!("download failed: {error}"))
}

fn decode_data_url(url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let (header, payload) = url
        .split_once(',')
        .ok_or_else(|| "invalid data URL".to_owned())?;
    if !header.to_ascii_lowercase().ends_with(";base64") {
        return Err("only base64 data URLs are supported".to_owned());
    }

    let payload = payload
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    let decoded_length = u64::try_from(payload.len() / 4 * 3).unwrap_or(u64::MAX);
    if decoded_length > max_bytes {
        return Err("album art is too large".to_owned());
    }

    let bytes = BASE64
        .decode(payload)
        .map_err(|error| format!("invalid base64 in data URL: {error}"))?;
    ensure_within_limit(&bytes, max_bytes)?;
    Ok(bytes)
}

fn ensure_within_limit(bytes: &[u8], max_bytes: u64) -> Result<(), String> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        Err("album art is too large".to_owned())
    } else {
        Ok(())
    }
}

fn decode(bytes: Vec<u8>, max_dimension: u32) -> Result<LoadedArt, String> {
    if looks_like_svg(&bytes) {
        return Ok(LoadedArt {
            handle: icon::from_svg_bytes(bytes),
            color: None,
        });
    }

    let mut reader = ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|error| format!("unable to read album art: {error}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_DIMENSION);
    limits.max_image_height = Some(MAX_DECODED_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);

    let image = reader
        .decode()
        .map_err(|error| format!("unable to decode album art: {error}"))?;
    let color = Some(dominant_album_color(&image));
    let pixels = display_pixels(image, max_dimension);
    let (width, height) = pixels.dimensions();

    Ok(LoadedArt {
        handle: icon::from_raster_pixels(width, height, pixels.into_raw()),
        color,
    })
}

/// Downscales large covers (never upscales) to keep cached covers small.
fn display_pixels(image: DynamicImage, max_dimension: u32) -> RgbaImage {
    if image.width() > max_dimension || image.height() > max_dimension {
        image.thumbnail(max_dimension, max_dimension).into_rgba8()
    } else {
        image.into_rgba8()
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let head = String::from_utf8_lossy(head);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))
}

fn file_url_to_path(value: &str) -> Option<PathBuf> {
    if !has_valid_percent_encoding(value) {
        return None;
    }

    Url::parse(value).ok()?.to_file_path().ok()
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba};
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;
    use tempfile::tempdir;

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbaImage::from_pixel(width, height, Rgba([220, 30, 30, 255]))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn file_url(path: &std::path::Path) -> String {
        Url::from_file_path(path).unwrap().to_string()
    }

    /// Serves a single HTTP response on a local port and returns its URL.
    fn serve_once(status: &'static str, body: Vec<u8>, delay: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            thread::sleep(delay);
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{address}/cover")
    }

    #[test]
    fn parses_local_file_urls() {
        assert_eq!(
            file_url_to_path("file:///home/user/Music/Album%20Art.png"),
            Some(PathBuf::from("/home/user/Music/Album Art.png"))
        );
        assert_eq!(
            file_url_to_path("file://localhost/home/user/M%C3%BAsica.png"),
            Some(PathBuf::from("/home/user/Música.png"))
        );
    }

    #[test]
    fn parses_unescaped_unicode_file_urls() {
        assert_eq!(
            file_url_to_path("file:///home/user/Música/Portada.png"),
            Some(PathBuf::from("/home/user/Música/Portada.png"))
        );
    }

    #[test]
    fn rejects_remote_non_file_and_malformed_urls() {
        assert_eq!(file_url_to_path("https://example.com/cover.png"), None);
        assert_eq!(
            file_url_to_path("file://example.com/home/user/cover.png"),
            None
        );
        assert_eq!(file_url_to_path("file:///home/user/bad%2.png"), None);
        assert_eq!(file_url_to_path("file:///home/user/bad%ZZ.png"), None);
    }

    #[test]
    fn loads_local_file_without_extension() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("cover");
        std::fs::write(&path, png_bytes(4, 4)).unwrap();

        let art = load(&file_url(&path)).unwrap();
        assert!(art.color.is_some());
    }

    #[test]
    fn rejects_missing_and_oversized_files() {
        let directory = tempdir().unwrap();
        assert!(load(&file_url(&directory.path().join("missing.png"))).is_err());

        let path = directory.path().join("huge.png");
        File::create(&path).unwrap().set_len(MAX_BYTES + 1).unwrap();
        let error = load(&file_url(&path)).unwrap_err();
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn loads_base64_data_urls() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes(4, 4));
        assert!(load(&format!("data:image/png;base64,{encoded}")).is_ok());
        assert!(
            load(&format!(
                "DATA:image/png;BASE64,{}",
                encoded.trim_end_matches('=')
            ))
            .is_ok()
        );

        let (start, end) = encoded.split_at(encoded.len() / 2);
        assert!(load(&format!("data:image/png;base64,{start}\n {end}")).is_ok());
    }

    #[test]
    fn rejects_invalid_data_urls() {
        assert!(load("data:image/png;base64,not*base64").is_err());
        assert!(load("data:image/png,rawbytes").is_err());
        assert!(load("data:image/png;base64").is_err());
    }

    #[test]
    fn rejects_unsupported_urls_and_non_images() {
        for url in [
            "",
            "ftp://example.com/cover.png",
            "cover.png",
            "/tmp/cover.png",
        ] {
            assert!(load(url).is_err(), "{url}");
        }

        let encoded = base64::engine::general_purpose::STANDARD.encode(b"not an image");
        assert!(load(&format!("data:image/png;base64,{encoded}")).is_err());
    }

    #[test]
    fn loads_svg_without_album_color() {
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(svg);
        let art = load(&format!("data:image/svg+xml;base64,{encoded}")).unwrap();
        assert!(art.color.is_none());
    }

    #[test]
    fn downloads_http_covers() {
        let url = serve_once("200 OK", png_bytes(4, 4), Duration::ZERO);
        assert!(load(&url).is_ok());
    }

    #[test]
    fn rejects_http_errors_oversized_bodies_and_timeouts() {
        let url = serve_once("404 Not Found", Vec::new(), Duration::ZERO);
        assert!(load(&url).is_err());

        let options = LoadOptions {
            max_bytes: 16,
            ..LoadOptions::default()
        };
        let url = serve_once("200 OK", png_bytes(4, 4), Duration::ZERO);
        assert!(load_with(&url, &options).is_err());

        let options = LoadOptions {
            timeout: Duration::from_millis(200),
            ..LoadOptions::default()
        };
        let url = serve_once("200 OK", png_bytes(4, 4), Duration::from_secs(2));
        assert!(load_with(&url, &options).is_err());
    }

    #[test]
    fn large_covers_are_downscaled_but_small_ones_are_kept() {
        let large = DynamicImage::ImageRgba8(RgbaImage::new(600, 300));
        assert_eq!(display_pixels(large, 256).dimensions(), (256, 128));

        let small = DynamicImage::ImageRgba8(RgbaImage::new(64, 64));
        assert_eq!(display_pixels(small, 256).dimensions(), (64, 64));
    }

    #[test]
    fn redacts_long_urls() {
        assert_eq!(redact("file:///a.png"), "file:///a.png");
        let long = format!("data:image/png;base64,{}", "A".repeat(500));
        assert_eq!(redact(&long).chars().count(), REDACTED_LENGTH + 1);
    }

    #[test]
    fn detects_file_urls() {
        assert!(is_file_url("file:///tmp/cover.png"));
        assert!(is_file_url("FILE:///tmp/cover.png"));
        assert!(!is_file_url("https://i.scdn.co/image/abc"));
    }
}
