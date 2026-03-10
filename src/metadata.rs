use std::path::PathBuf;

use mpris::PlayerFinder;

use crate::fl;
use crate::player::{album_art_path_from_metadata, playback_state_from_player};
use crate::window::PlaybackState;

#[derive(Clone, Debug)]
pub struct NowPlayingData {
    pub text: String,
    pub title: String,
    pub artist: String,
    pub state: PlaybackState,
    pub album_art_path: Option<PathBuf>,
    pub has_active_media: bool,
}

pub fn now_playing_snapshot() -> NowPlayingData {
    let finder = PlayerFinder::new();

    if let Ok(finder) = finder {
        if let Ok(player) = finder.find_active() {
            return now_playing_from_player(&player);
        }
    }

    NowPlayingData {
        text: fl!("nothing-playing"),
        title: fl!("nothing-playing"),
        artist: String::new(),
        state: PlaybackState::Stopped,
        album_art_path: None,
        has_active_media: false,
    }
}

pub fn now_playing_from_player(player: &mpris::Player) -> NowPlayingData {
    let playback_state = playback_state_from_player(player);

    if let Ok(meta) = player.get_metadata() {
        let title = meta
            .title()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fl!("unknown-title"));
        let artist = meta
            .artists()
            .and_then(|a| a.first().copied())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fl!("unknown-artist"));
        let album_art_path = album_art_path_from_metadata(&meta);

        return NowPlayingData {
            text: format!("{} - {}", title, artist),
            title,
            artist,
            state: playback_state,
            album_art_path,
            has_active_media: true,
        };
    }

    NowPlayingData {
        text: fl!("nothing-playing"),
        title: fl!("nothing-playing"),
        artist: String::new(),
        state: playback_state,
        album_art_path: None,
        has_active_media: false,
    }
}
