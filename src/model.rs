use std::path::PathBuf;

use cosmic::iced::Color;

use crate::album_color::dominant_album_color;
use crate::fl;
use crate::settings::AppSettings;

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
    pub album_art_path: Option<PathBuf>,
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
            album_art_path: None,
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
    pub album_color: Option<Color>,
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
            album_color: None,
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
        self.refresh_album_color();
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

    fn refresh_now_playing(&mut self) {
        self.now_playing = self
            .selected_player
            .as_ref()
            .and_then(|selected| self.players.iter().find(|player| &player.id == selected))
            .map_or_else(NowPlayingData::nothing_playing, |player| {
                player.now_playing.clone()
            });
        self.refresh_album_color();
    }

    fn refresh_album_color(&mut self) {
        self.album_color = if self.settings.album_color_enabled {
            self.now_playing
                .album_art_path
                .as_deref()
                .and_then(dominant_album_color)
        } else {
            None
        };
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
                album_art_path: None,
                has_usable_metadata: has_metadata,
            },
        }
    }

    fn snapshot(players: Vec<PlayerInfo>) -> MediaSnapshot {
        MediaSnapshot { players }
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
}
