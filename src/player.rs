use std::path::PathBuf;

use mpris::{PlaybackStatus, PlayerFinder};

use crate::window::PlaybackState;

pub fn playback_state_from_player(player: &mpris::Player) -> PlaybackState {
    match player.get_playback_status() {
        Ok(PlaybackStatus::Playing) => PlaybackState::Playing,
        Ok(PlaybackStatus::Paused) => PlaybackState::Paused,
        Ok(PlaybackStatus::Stopped) => PlaybackState::Stopped,
        Err(_) => PlaybackState::Unknown,
    }
}

pub fn album_art_path_from_metadata(meta: &mpris::Metadata) -> Option<PathBuf> {
    let art_url = meta.art_url()?;
    file_url_to_path(art_url)
}

fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let path = url
        .strip_prefix("file://localhost")
        .or_else(|| url.strip_prefix("file://"))?;

    Some(PathBuf::from(percent_decode_path(path)))
}

fn percent_decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                decoded.push((high << 4) | low);
                i += 3;
                continue;
            }
        }

        decoded.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::file_url_to_path;
    use std::path::PathBuf;

    #[test]
    fn parses_file_url_paths() {
        assert_eq!(
            file_url_to_path("file:///home/user/Music/Album%20Art.png"),
            Some(PathBuf::from("/home/user/Music/Album Art.png"))
        );
    }

    #[test]
    fn ignores_non_file_urls() {
        assert_eq!(file_url_to_path("https://example.com/cover.png"), None);
    }
}

pub fn with_active_player<F>(f: F)
where
    F: FnOnce(&mpris::Player),
{
    if let Ok(finder) = PlayerFinder::new() {
        if let Ok(player) = finder.find_active() {
            f(&player);
        }
    }
}
