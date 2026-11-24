use std::collections::{HashSet, VecDeque};
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

use image::imageops::FilterType;
use image::{ImageFormat, ImageReader, Limits};
use tempfile::TempDir;
use ureq::Agent;

const CACHE_CAPACITY: usize = 32;
const MAX_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INPUT_DIMENSION: u32 = 4096;
const MAX_OUTPUT_DIMENSION: u32 = 1024;
const MAX_DECODE_ALLOCATION: u64 = 96 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRequest {
    pub url: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct DownloadCompletion {
    pub url: String,
    pub result: Result<PathBuf, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Resolution {
    Cached(PathBuf),
    Download(DownloadRequest),
    Pending,
}

#[derive(Debug)]
struct CacheEntry {
    url: String,
    path: PathBuf,
}

#[derive(Debug)]
pub struct ArtworkCache {
    directory: TempDir,
    entries: VecDeque<CacheEntry>,
    in_flight: HashSet<String>,
    next_file_id: u64,
}

impl ArtworkCache {
    pub fn new() -> Result<Self, String> {
        let directory = tempfile::Builder::new()
            .prefix("cosmic-now-playing-art-")
            .tempdir()
            .map_err(|error| format!("unable to create artwork cache: {error}"))?;

        Ok(Self {
            directory,
            entries: VecDeque::new(),
            in_flight: HashSet::new(),
            next_file_id: 0,
        })
    }

    pub fn resolve(&mut self, url: &str) -> Resolution {
        if let Some(index) = self.entries.iter().position(|entry| entry.url == url) {
            let entry = self.entries.remove(index).expect("cache entry exists");
            let path = entry.path.clone();
            self.entries.push_back(entry);
            return Resolution::Cached(path);
        }

        if !self.in_flight.insert(url.to_owned()) {
            return Resolution::Pending;
        }

        let path = self
            .directory
            .path()
            .join(format!("cover-{}.png", self.next_file_id));
        self.next_file_id = self.next_file_id.wrapping_add(1);

        Resolution::Download(DownloadRequest {
            url: url.to_owned(),
            path,
        })
    }

    pub fn complete(
        &mut self,
        completion: &DownloadCompletion,
        keep: bool,
    ) -> Option<PathBuf> {
        self.in_flight.remove(&completion.url);

        let Ok(path) = &completion.result else {
            return None;
        };

        if !keep {
            if let Err(error) = std::fs::remove_file(path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("unable to remove stale artwork {}: {error}", path.display());
            }
            return None;
        }

        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.url == completion.url)
        {
            let old_entry = self.entries.remove(index).expect("cache entry exists");
            if old_entry.path != *path {
                let _ = std::fs::remove_file(old_entry.path);
            }
        }

        self.entries.push_back(CacheEntry {
            url: completion.url.clone(),
            path: path.clone(),
        });
        self.evict_old_entries();
        Some(path.clone())
    }

    fn evict_old_entries(&mut self) {
        while self.entries.len() > CACHE_CAPACITY {
            if let Some(entry) = self.entries.pop_front()
                && let Err(error) = std::fs::remove_file(&entry.path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "unable to remove cached artwork {}: {error}",
                    entry.path.display()
                );
            }
        }
    }
}

#[must_use]
pub fn download(request: DownloadRequest) -> DownloadCompletion {
    let result = download_inner(&request);
    if result.is_err() {
        let _ = std::fs::remove_file(&request.path);
    }
    DownloadCompletion {
        url: request.url,
        result,
    }
}

fn download_inner(request: &DownloadRequest) -> Result<PathBuf, String> {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(&request.url)
        .call()
        .map_err(|error| format!("request failed: {error}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .map_err(|error| format!("unable to read response: {error}"))?;

    let format = image::guess_format(&bytes)
        .map_err(|error| format!("response is not a recognized image: {error}"))?;
    if !matches!(format, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP) {
        return Err(format!("unsupported image format: {format:?}"));
    }

    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_INPUT_DIMENSION);
    limits.max_image_height = Some(MAX_INPUT_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("unable to decode image: {error}"))?;
    let image = image.resize(
        MAX_OUTPUT_DIMENSION,
        MAX_OUTPUT_DIMENSION,
        FilterType::Lanczos3,
    );
    image
        .save_with_format(&request.path, ImageFormat::Png)
        .map_err(|error| format!("unable to store image: {error}"))?;

    Ok(request.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn completion(url: &str, path: PathBuf) -> DownloadCompletion {
        std::fs::write(&path, b"cached").unwrap();
        DownloadCompletion {
            url: url.to_owned(),
            result: Ok(path),
        }
    }

    fn serve_with_headers(status: &str, headers: String, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });
        format!("http://{address}/cover")
    }

    fn serve_once(status: &str, body: Vec<u8>) -> String {
        serve_with_headers(status, String::new(), body)
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([30, 80, 180, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode test image");
        bytes.into_inner()
    }

    #[test]
    fn deduplicates_requests_and_reuses_cached_files() {
        let mut cache = ArtworkCache::new().unwrap();
        let Resolution::Download(request) = cache.resolve("https://example.com/a") else {
            panic!("first lookup should download");
        };
        assert_eq!(
            cache.resolve("https://example.com/a"),
            Resolution::Pending
        );

        let completion = completion("https://example.com/a", request.path.clone());
        assert_eq!(cache.complete(&completion, true), Some(request.path.clone()));
        assert_eq!(
            cache.resolve("https://example.com/a"),
            Resolution::Cached(request.path)
        );
    }

    #[test]
    fn discards_stale_downloads() {
        let mut cache = ArtworkCache::new().unwrap();
        let Resolution::Download(request) = cache.resolve("https://example.com/a") else {
            panic!("lookup should download");
        };
        let completion = completion("https://example.com/a", request.path.clone());

        assert_eq!(cache.complete(&completion, false), None);
        assert!(!request.path.exists());
        assert!(matches!(
            cache.resolve("https://example.com/a"),
            Resolution::Download(_)
        ));
    }

    #[test]
    fn evicts_least_recently_used_entry() {
        let mut cache = ArtworkCache::new().unwrap();
        let mut first_path = None;
        for index in 0..=CACHE_CAPACITY {
            let url = format!("https://example.com/{index}");
            let Resolution::Download(request) = cache.resolve(&url) else {
                panic!("unique URL should download");
            };
            if index == 0 {
                first_path = Some(request.path.clone());
            }
            cache.complete(&completion(&url, request.path), true);
        }

        let first_path = first_path.unwrap();
        assert!(!first_path.exists());
        assert_eq!(cache.entries.len(), CACHE_CAPACITY);
    }

    #[test]
    fn downloads_and_normalizes_supported_images() {
        let directory = tempfile::tempdir().unwrap();
        let request = DownloadRequest {
            url: serve_once("200 OK", png_bytes(1200, 600)),
            path: directory.path().join("cover.png"),
        };

        let completion = download(request);
        let path = completion.result.unwrap();
        let image = image::open(path).unwrap();
        assert_eq!((image.width(), image.height()), (1024, 512));
    }

    #[test]
    fn follows_http_redirects() {
        let directory = tempfile::tempdir().unwrap();
        let destination = serve_once("200 OK", png_bytes(2, 2));
        let redirect = serve_with_headers(
            "302 Found",
            format!("Location: {destination}\r\n"),
            Vec::new(),
        );
        let completion = download(DownloadRequest {
            url: redirect,
            path: directory.path().join("cover.png"),
        });

        assert!(completion.result.is_ok());
    }

    #[test]
    fn rejects_http_errors_invalid_images_and_excessive_dimensions() {
        let directory = tempfile::tempdir().unwrap();
        for (index, status, body) in [
            (0, "404 Not Found", png_bytes(1, 1)),
            (1, "200 OK", b"not an image".to_vec()),
            (
                2,
                "200 OK",
                png_bytes(MAX_INPUT_DIMENSION + 1, 1),
            ),
        ] {
            let completion = download(DownloadRequest {
                url: serve_once(status, body),
                path: directory.path().join(format!("cover-{index}.png")),
            });
            assert!(completion.result.is_err());
            assert!(!directory
                .path()
                .join(format!("cover-{index}.png"))
                .exists());
        }
    }

    #[test]
    fn rejects_oversized_responses() {
        let directory = tempfile::tempdir().unwrap();
        let oversized_length = usize::try_from(MAX_DOWNLOAD_BYTES).unwrap() + 1;
        let completion = download(DownloadRequest {
            url: serve_once("200 OK", vec![0; oversized_length]),
            path: directory.path().join("cover.png"),
        });

        assert!(completion.result.is_err());
    }
}
