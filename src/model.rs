use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use cosmic::iced::Color;
use cosmic::widget::icon;

use crate::album_art::{self, LoadedArt};
use crate::fl;
use crate::settings::AppSettings;

const ART_CACHE_CAPACITY: usize = 8;
/// Failed covers are retried after this delay (e.g. when the network was down).
const ART_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NowPlayingData {
    pub text: String,
    pub title: String,
    pub artist: String,
    pub state: PlaybackState,
    pub art_url: Option<String>,
    pub has_usable_metadata: bool,
}

impl NowPlayingData {
    #[must_use]
    pub fn nothing_playing() -> Self {
        Self {
            text: fl!("nothing-playing"),
            title: fl!("nothing-playing"),
            artist: String::new(),
            state: PlaybackState::Stopped,
            art_url: None,
            has_usable_metadata: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerInfo {
    pub id: String,
    pub identity: String,
    pub now_playing: NowPlayingData,
}

impl PlayerInfo {
    #[must_use]
    pub fn track(&self) -> &str {
        if self.now_playing.has_usable_metadata {
            &self.now_playing.text
        } else {
            ""
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MediaSnapshot {
    pub players: Vec<PlayerInfo>,
}

/// Identifies a cover to load. Local files are also keyed by track, because
/// some players reuse the same path for every cover.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ArtKey {
    pub url: String,
    track: Option<String>,
}

#[derive(Debug)]
struct CachedArt {
    key: ArtKey,
    art: Option<LoadedArt>,
    loaded_at: Instant,
}

impl CachedArt {
    fn should_retry(&self) -> bool {
        self.art.is_none() && self.loaded_at.elapsed() >= ART_RETRY_DELAY
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PopupPage {
    #[default]
    Main,
    PlayerList,
    Settings,
}

#[derive(Debug)]
pub struct AppModel {
    pub now_playing: NowPlayingData,
    pub players: Vec<PlayerInfo>,
    pub selected_player: Option<String>,
    pub popup_page: PopupPage,
    pub settings: AppSettings,
    artwork: Option<(ArtKey, LoadedArt)>,
    art_cache: VecDeque<CachedArt>,
    art_pending: HashSet<ArtKey>,
}

impl AppModel {
    #[must_use]
    pub fn new(settings: AppSettings, snapshot: MediaSnapshot) -> Self {
        let mut model = Self {
            now_playing: NowPlayingData::nothing_playing(),
            players: Vec::new(),
            selected_player: None,
            popup_page: PopupPage::Main,
            settings,
            artwork: None,
            art_cache: VecDeque::new(),
            art_pending: HashSet::new(),
        };
        model.apply_snapshot(snapshot);
        model
    }

    pub fn apply_snapshot(&mut self, snapshot: MediaSnapshot) {
        let selected_is_available = self
            .selected_player
            .as_ref()
            .is_some_and(|selected| snapshot.players.iter().any(|player| &player.id == selected));

        if !selected_is_available {
            self.selected_player =
                preferred_player(&snapshot.players).map(|player| player.id.clone());
        }

        self.players = snapshot.players;
        self.refresh_now_playing();
    }

    pub fn select_player(&mut self, id: String) {
        if self.players.iter().any(|player| player.id == id) {
            self.selected_player = Some(id);
            self.refresh_now_playing();
        }
    }

    pub fn set_popup_page(&mut self, page: PopupPage) {
        self.popup_page = page;
    }

    pub fn set_album_color_enabled(&mut self, enabled: bool) {
        self.settings.album_color_enabled = enabled;
    }

    pub fn set_previous_control_enabled(&mut self, enabled: bool) {
        self.settings.previous_control_enabled = enabled;
    }

    pub fn set_play_control_enabled(&mut self, enabled: bool) {
        self.settings.play_control_enabled = enabled;
    }

    pub fn set_next_control_enabled(&mut self, enabled: bool) {
        self.settings.next_control_enabled = enabled;
    }

    #[must_use]
    pub fn has_active_media(&self) -> bool {
        self.now_playing.has_usable_metadata
    }

    #[must_use]
    pub fn artwork(&self) -> Option<&icon::Handle> {
        self.artwork.as_ref().map(|(_, art)| &art.handle)
    }

    #[must_use]
    pub fn album_color(&self) -> Option<Color> {
        if !self.settings.album_color_enabled {
            return None;
        }
        self.artwork.as_ref().and_then(|(_, art)| art.color)
    }

    /// Returns the cover that still has to be loaded, at most once per cover.
    pub fn take_artwork_request(&mut self) -> Option<ArtKey> {
        let key = self.current_art_key()?;
        let is_shown = self
            .artwork
            .as_ref()
            .is_some_and(|(shown, _)| *shown == key);
        let is_cached = self
            .art_cache
            .iter()
            .any(|cached| cached.key == key && !cached.should_retry());
        if is_shown || is_cached || self.art_pending.contains(&key) {
            return None;
        }

        self.art_pending.insert(key.clone());
        Some(key)
    }

    /// Stores a loaded cover; it is only shown if the track still uses it.
    pub fn art_loaded(&mut self, key: ArtKey, result: Result<LoadedArt, String>) {
        self.art_pending.remove(&key);
        let art = result.ok();
        if self.current_art_key().as_ref() == Some(&key) {
            self.artwork = art.clone().map(|art| (key.clone(), art));
        }

        self.art_cache.retain(|cached| cached.key != key);
        if self.art_cache.len() >= ART_CACHE_CAPACITY {
            self.art_cache.pop_front();
        }
        self.art_cache.push_back(CachedArt {
            key,
            art,
            loaded_at: Instant::now(),
        });
    }

    fn refresh_now_playing(&mut self) {
        self.now_playing = self
            .selected_player
            .as_ref()
            .and_then(|selected| self.players.iter().find(|player| &player.id == selected))
            .map_or_else(NowPlayingData::nothing_playing, |player| {
                player.now_playing.clone()
            });
        self.sync_artwork();
    }

    fn current_art_key(&self) -> Option<ArtKey> {
        let url = self.now_playing.art_url.as_ref()?;
        let track = album_art::is_file_url(url).then(|| self.now_playing.text.clone());
        Some(ArtKey {
            url: url.clone(),
            track,
        })
    }

    fn sync_artwork(&mut self) {
        let Some(key) = self.current_art_key() else {
            self.artwork = None;
            return;
        };
        if self
            .artwork
            .as_ref()
            .is_some_and(|(shown, _)| *shown == key)
        {
            return;
        }

        let cached = self
            .touch_cached(&key)
            .and_then(|cached| cached.art.clone());
        self.artwork = cached.map(|art| (key, art));
    }

    /// Looks up a cover and marks it as most recently used.
    fn touch_cached(&mut self, key: &ArtKey) -> Option<&CachedArt> {
        let index = self
            .art_cache
            .iter()
            .position(|cached| cached.key == *key)?;
        let cached = self.art_cache.remove(index)?;
        self.art_cache.push_back(cached);
        self.art_cache.back()
    }
}

fn preferred_player(players: &[PlayerInfo]) -> Option<&PlayerInfo> {
    players
        .iter()
        .find(|player| player.now_playing.state == PlaybackState::Playing)
        .or_else(|| {
            players
                .iter()
                .find(|player| player.now_playing.has_usable_metadata)
        })
        .or_else(|| players.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: &str, state: PlaybackState, has_metadata: bool) -> PlayerInfo {
        PlayerInfo {
            id: id.to_owned(),
            identity: id.to_owned(),
            now_playing: NowPlayingData {
                text: id.to_owned(),
                title: id.to_owned(),
                artist: "Artist".to_owned(),
                state,
                art_url: None,
                has_usable_metadata: has_metadata,
            },
        }
    }

    fn snapshot(players: Vec<PlayerInfo>) -> MediaSnapshot {
        MediaSnapshot { players }
    }

    fn player_with_art(id: &str, track: &str, art_url: &str) -> PlayerInfo {
        let mut player = player(id, PlaybackState::Playing, true);
        player.now_playing.text = track.to_owned();
        player.now_playing.art_url = Some(art_url.to_owned());
        player
    }

    fn art(color: Color) -> LoadedArt {
        LoadedArt {
            handle: icon::from_raster_pixels(1, 1, vec![0, 0, 0, 255]),
            color: Some(color),
        }
    }

    fn model_playing(art_url: &str) -> AppModel {
        AppModel::new(
            AppSettings::default(),
            snapshot(vec![player_with_art("player", "Track", art_url)]),
        )
    }

    #[test]
    fn selects_playing_player_before_other_players() {
        let model = AppModel::new(
            AppSettings::default(),
            snapshot(vec![
                player("paused", PlaybackState::Paused, true),
                player("playing", PlaybackState::Playing, true),
            ]),
        );

        assert_eq!(model.selected_player.as_deref(), Some("playing"));
    }

    #[test]
    fn selects_player_with_metadata_before_empty_player() {
        let model = AppModel::new(
            AppSettings::default(),
            snapshot(vec![
                player("empty", PlaybackState::Stopped, false),
                player("paused", PlaybackState::Paused, true),
            ]),
        );

        assert_eq!(model.selected_player.as_deref(), Some("paused"));
        assert!(model.has_active_media());
    }

    #[test]
    fn preserves_selection_until_player_disappears() {
        let mut model = AppModel::new(
            AppSettings::default(),
            snapshot(vec![
                player("one", PlaybackState::Playing, true),
                player("two", PlaybackState::Paused, true),
            ]),
        );
        model.select_player("two".to_owned());
        model.apply_snapshot(snapshot(vec![
            player("one", PlaybackState::Playing, true),
            player("two", PlaybackState::Stopped, true),
        ]));
        assert_eq!(model.selected_player.as_deref(), Some("two"));

        model.apply_snapshot(snapshot(vec![player("one", PlaybackState::Paused, true)]));
        assert_eq!(model.selected_player.as_deref(), Some("one"));
    }

    #[test]
    fn paused_and_stopped_tracks_remain_visible_with_metadata() {
        for state in [PlaybackState::Paused, PlaybackState::Stopped] {
            let model = AppModel::new(
                AppSettings::default(),
                snapshot(vec![player("track", state, true)]),
            );
            assert!(model.has_active_media());
        }
    }

    #[test]
    fn popup_page_has_only_one_active_value() {
        let mut model = AppModel::new(AppSettings::default(), MediaSnapshot::default());
        model.set_popup_page(PopupPage::PlayerList);
        assert_eq!(model.popup_page, PopupPage::PlayerList);
        model.set_popup_page(PopupPage::Settings);
        assert_eq!(model.popup_page, PopupPage::Settings);
        model.set_popup_page(PopupPage::Main);
        assert_eq!(model.popup_page, PopupPage::Main);
    }

    #[test]
    fn requests_each_cover_once_until_it_loads() {
        let mut model = model_playing("https://example.com/a.jpg");

        let key = model.take_artwork_request().unwrap();
        assert_eq!(key.url, "https://example.com/a.jpg");
        assert_eq!(model.take_artwork_request(), None);

        model.apply_snapshot(snapshot(vec![player_with_art(
            "player",
            "Track",
            "https://example.com/a.jpg",
        )]));
        assert_eq!(model.take_artwork_request(), None);

        model.art_loaded(key, Ok(art(Color::BLACK)));
        assert!(model.artwork().is_some());
        assert_eq!(model.album_color(), Some(Color::BLACK));
        assert_eq!(model.take_artwork_request(), None);
    }

    #[test]
    fn stale_covers_are_cached_but_not_shown() {
        let mut model = model_playing("https://example.com/a.jpg");
        let first = model.take_artwork_request().unwrap();

        model.apply_snapshot(snapshot(vec![player_with_art(
            "player",
            "Next",
            "https://example.com/b.jpg",
        )]));
        model.art_loaded(first, Ok(art(Color::BLACK)));
        assert!(model.artwork().is_none());

        model.apply_snapshot(snapshot(vec![player_with_art(
            "player",
            "Track",
            "https://example.com/a.jpg",
        )]));
        assert!(model.artwork().is_some());
        assert_eq!(model.take_artwork_request(), None);
    }

    #[test]
    fn keeps_cover_when_another_player_uses_the_same_url() {
        let url = "https://example.com/a.jpg";
        let mut model = AppModel::new(
            AppSettings::default(),
            snapshot(vec![
                player_with_art("one", "Track", url),
                player_with_art("two", "Track", url),
            ]),
        );
        let key = model.take_artwork_request().unwrap();
        model.art_loaded(key, Ok(art(Color::BLACK)));

        model.select_player("two".to_owned());
        assert!(model.artwork().is_some());
        assert_eq!(model.take_artwork_request(), None);
    }

    #[test]
    fn failed_covers_are_not_requested_again_immediately() {
        let mut model = model_playing("https://example.com/a.jpg");
        let key = model.take_artwork_request().unwrap();
        model.art_loaded(key, Err("offline".to_owned()));

        assert!(model.artwork().is_none());
        assert_eq!(model.album_color(), None);
        assert_eq!(model.take_artwork_request(), None);
    }

    #[test]
    fn local_covers_are_reloaded_when_the_track_changes() {
        let url = "file:///tmp/cover.png";
        let mut model = model_playing(url);
        let key = model.take_artwork_request().unwrap();
        model.art_loaded(key, Ok(art(Color::BLACK)));

        model.apply_snapshot(snapshot(vec![player_with_art("player", "Next", url)]));
        assert!(model.artwork().is_none());
        assert!(model.take_artwork_request().is_some());
    }

    #[test]
    fn album_color_setting_does_not_reload_cover() {
        let mut model = model_playing("https://example.com/a.jpg");
        let key = model.take_artwork_request().unwrap();
        model.art_loaded(key, Ok(art(Color::BLACK)));

        model.set_album_color_enabled(false);
        assert_eq!(model.album_color(), None);
        assert!(model.artwork().is_some());
        assert_eq!(model.take_artwork_request(), None);

        model.set_album_color_enabled(true);
        assert_eq!(model.album_color(), Some(Color::BLACK));
    }

    #[test]
    fn cache_evicts_least_recently_used_cover() {
        let url = |index: usize| format!("https://example.com/{index}.jpg");
        let mut model = model_playing(&url(0));
        for index in 0..=ART_CACHE_CAPACITY {
            model.apply_snapshot(snapshot(vec![player_with_art(
                "player",
                "Track",
                &url(index),
            )]));
            let key = model.take_artwork_request().unwrap();
            model.art_loaded(key, Ok(art(Color::BLACK)));
        }

        model.apply_snapshot(snapshot(vec![player_with_art("player", "Track", &url(0))]));
        assert!(model.artwork().is_none());
        assert!(model.take_artwork_request().is_some());

        model.apply_snapshot(snapshot(vec![player_with_art(
            "player",
            "Track",
            &url(ART_CACHE_CAPACITY),
        )]));
        assert!(model.artwork().is_some());
    }

    #[test]
    fn missing_art_url_clears_cover() {
        let mut model = model_playing("https://example.com/a.jpg");
        let key = model.take_artwork_request().unwrap();
        model.art_loaded(key, Ok(art(Color::BLACK)));

        let mut without_art = player_with_art("player", "Track", "");
        without_art.now_playing.art_url = None;
        model.apply_snapshot(snapshot(vec![without_art]));
        assert!(model.artwork().is_none());
        assert_eq!(model.take_artwork_request(), None);
    }
}
