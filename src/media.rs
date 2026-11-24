use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc as std_mpsc};
use std::thread;
use std::time::Duration;

use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::futures::{SinkExt, executor::block_on};
use cosmic::iced::{Subscription, stream::channel};
use dbus::arg::PropMap;
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;
use dbus::message::MatchRule;
use mpris::{PlaybackStatus, PlayerFinder, TrackID};
use url::Url;

use crate::fl;
use crate::model::{ArtworkSource, MediaSnapshot, NowPlayingData, PlaybackState, PlayerInfo};

const LISTENER_WAKE_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_PLAYER_PREFIX: &str = "org.mpris.MediaPlayer2.";
const METADATA_PROPERTY: &str = "Metadata";
const PLAYBACK_STATUS_PROPERTY: &str = "PlaybackStatus";

#[derive(Clone, Copy, Debug)]
pub enum MediaCommand {
    Previous,
    TogglePlayPause,
    Next,
}

#[must_use]
pub fn initial_snapshot() -> MediaSnapshot {
    let finder = match PlayerFinder::new() {
        Ok(finder) => finder,
        Err(error) => {
            eprintln!("unable to connect to MPRIS: {error}");
            return MediaSnapshot::default();
        }
    };

    match scan(&finder) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("unable to read initial MPRIS state: {error}");
            MediaSnapshot::default()
        }
    }
}

pub fn subscription() -> Subscription<MediaSnapshot> {
    Subscription::run(monitor)
}

type CommandRequest = (String, MediaCommand);

static COMMAND_QUEUE: OnceLock<std_mpsc::Sender<CommandRequest>> = OnceLock::new();

/// Queues a command for the selected player without blocking the caller.
///
/// Commands run in order on a dedicated thread, because slow players can take
/// a while to answer over D-Bus.
pub fn enqueue_command(selected_player: Option<&str>, command: MediaCommand) {
    let Some(selected_player) = selected_player else {
        eprintln!("no media player is selected");
        return;
    };

    let queue = COMMAND_QUEUE.get_or_init(spawn_command_worker);
    if queue.send((selected_player.to_owned(), command)).is_err() {
        eprintln!("unable to run MPRIS command {command:?}: command worker is not running");
    }
}

fn spawn_command_worker() -> std_mpsc::Sender<CommandRequest> {
    let (sender, receiver) = std_mpsc::channel();
    if let Err(error) = thread::Builder::new()
        .name("media-commands".to_owned())
        .spawn(move || command_loop(&receiver))
    {
        eprintln!("unable to start MPRIS command worker: {error}");
    }
    sender
}

fn command_loop(receiver: &std_mpsc::Receiver<CommandRequest>) {
    for (selected_player, command) in receiver {
        if let Err(error) = run_command(&selected_player, command) {
            eprintln!("{error}");
        }
    }
}

// A fresh connection per command: reusing one PlayerFinder made back-to-back
// commands to slow players time out.
fn run_command(selected_player: &str, command: MediaCommand) -> Result<(), String> {
    let finder =
        PlayerFinder::new().map_err(|error| format!("unable to connect to MPRIS: {error}"))?;
    let players = finder
        .find_all()
        .map_err(|error| format!("unable to list MPRIS players: {error}"))?;
    let player = players
        .iter()
        .find(|player| player.bus_name() == selected_player)
        .ok_or_else(|| format!("selected MPRIS player disappeared: {selected_player}"))?;

    let result = match command {
        MediaCommand::Previous => player.previous(),
        MediaCommand::TogglePlayPause => player.play_pause(),
        MediaCommand::Next => player.next(),
    };

    result.map_err(|error| {
        format!(
            "MPRIS command {command:?} failed for {}: {error}",
            player.identity()
        )
    })
}

fn monitor() -> impl cosmic::iced::futures::Stream<Item = MediaSnapshot> {
    channel(8, |mut output: mpsc::Sender<MediaSnapshot>| async move {
        thread::spawn(move || monitor_loop(&mut output));
    })
}

fn monitor_loop(output: &mut mpsc::Sender<MediaSnapshot>) {
    let mut last_sent = None;

    loop {
        if output.is_closed() {
            break;
        }

        let event_monitor = match EventMonitor::new() {
            Ok(event_monitor) => event_monitor,
            Err(error) => {
                eprintln!("unable to listen for MPRIS events: {error}");
                thread::sleep(RECONNECT_INTERVAL);
                continue;
            }
        };

        let finder = match PlayerFinder::new() {
            Ok(finder) => finder,
            Err(error) => {
                eprintln!("unable to connect to MPRIS: {error}");
                thread::sleep(RECONNECT_INTERVAL);
                continue;
            }
        };

        let snapshot = match scan(&finder) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("unable to read MPRIS players: {error}");
                thread::sleep(RECONNECT_INTERVAL);
                continue;
            }
        };

        if !send_snapshot_if_changed(output, &mut last_sent, snapshot) {
            break;
        }

        loop {
            if output.is_closed() {
                return;
            }

            match event_monitor.process_next_batch() {
                Ok(false) => continue,
                Ok(true) => {}
                Err(error) => {
                    eprintln!("unable to process MPRIS events: {error}");
                    break;
                }
            }

            let snapshot = match scan(&finder) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    eprintln!("unable to refresh MPRIS players after an event: {error}");
                    break;
                }
            };

            if !send_snapshot_if_changed(output, &mut last_sent, snapshot) {
                return;
            }
        }

        thread::sleep(RECONNECT_INTERVAL);
    }
}

struct EventMonitor {
    connection: Connection,
    refresh_requested: Arc<AtomicBool>,
}

impl EventMonitor {
    fn new() -> Result<Self, dbus::Error> {
        let connection = Connection::new_session()?;
        let refresh_requested = Arc::new(AtomicBool::new(false));

        let properties_flag = Arc::clone(&refresh_requested);
        let properties_rule =
            MatchRule::new_signal("org.freedesktop.DBus.Properties", "PropertiesChanged")
                .with_path(MPRIS_PATH);
        connection.add_match(
            properties_rule,
            move |change: PropertiesPropertiesChanged, _, _| {
                if is_relevant_properties_change(
                    &change.interface_name,
                    &change.changed_properties,
                    &change.invalidated_properties,
                ) {
                    properties_flag.store(true, Ordering::Release);
                }
                true
            },
        )?;

        let owner_flag = Arc::clone(&refresh_requested);
        let owner_rule =
            MatchRule::new_signal("org.freedesktop.DBus", "NameOwnerChanged")
                .with_sender("org.freedesktop.DBus")
                .with_path("/org/freedesktop/DBus");
        connection.add_match(
            owner_rule,
            move |(name, old_owner, new_owner): (String, String, String), _, _| {
                if is_mpris_owner_change(&name, &old_owner, &new_owner) {
                    owner_flag.store(true, Ordering::Release);
                }
                true
            },
        )?;

        Ok(Self {
            connection,
            refresh_requested,
        })
    }

    fn process_next_batch(&self) -> Result<bool, dbus::Error> {
        self.connection.process(LISTENER_WAKE_INTERVAL)?;

        if !self.refresh_requested.load(Ordering::Acquire) {
            return Ok(false);
        }

        while self.connection.process(Duration::ZERO)? {}

        Ok(self.refresh_requested.swap(false, Ordering::AcqRel))
    }
}

fn is_relevant_properties_change(
    interface_name: &str,
    changed_properties: &PropMap,
    invalidated_properties: &[String],
) -> bool {
    interface_name == MPRIS_PLAYER_INTERFACE
        && [METADATA_PROPERTY, PLAYBACK_STATUS_PROPERTY]
            .iter()
            .any(|property| {
                changed_properties.contains_key(*property)
                    || invalidated_properties
                        .iter()
                        .any(|invalidated| invalidated == property)
            })
}

fn is_mpris_owner_change(name: &str, old_owner: &str, new_owner: &str) -> bool {
    name.starts_with(MPRIS_PLAYER_PREFIX) && old_owner != new_owner
}

fn send_snapshot_if_changed(
    output: &mut mpsc::Sender<MediaSnapshot>,
    last_sent: &mut Option<MediaSnapshot>,
    snapshot: MediaSnapshot,
) -> bool {
    if !snapshot_has_changed(last_sent, &snapshot) {
        return true;
    }

    if block_on(output.send(snapshot.clone())).is_err() {
        return false;
    }

    *last_sent = Some(snapshot);
    true
}

fn snapshot_has_changed(
    last_sent: &Option<MediaSnapshot>,
    snapshot: &MediaSnapshot,
) -> bool {
    last_sent.as_ref() != Some(snapshot)
}

fn scan(finder: &PlayerFinder) -> Result<MediaSnapshot, mpris::FindingError> {
    let players = finder
        .find_all()?
        .iter()
        .map(player_info)
        .collect::<Vec<_>>();
    Ok(MediaSnapshot { players })
}

fn player_info(player: &mpris::Player) -> PlayerInfo {
    PlayerInfo {
        id: player.bus_name().to_owned(),
        identity: player.identity().to_owned(),
        now_playing: now_playing(player),
    }
}

fn now_playing(player: &mpris::Player) -> NowPlayingData {
    let state = playback_state(player);
    let Ok(metadata) = player.get_metadata() else {
        return NowPlayingData {
            state,
            ..NowPlayingData::nothing_playing()
        };
    };

    if metadata.is_empty() || metadata.track_id() == Some(TrackID::no_track()) {
        return NowPlayingData {
            state,
            ..NowPlayingData::nothing_playing()
        };
    }

    let title = metadata
        .title()
        .map_or_else(|| fl!("unknown-title"), ToOwned::to_owned);
    let artist = metadata
        .artists()
        .and_then(|artists| artists.first().copied())
        .map_or_else(|| fl!("unknown-artist"), ToOwned::to_owned);

    NowPlayingData {
        text: format!("{title} - {artist}"),
        title,
        artist,
        state,
        artwork_source: metadata.art_url().and_then(artwork_source),
        has_usable_metadata: true,
    }
}

fn playback_state(player: &mpris::Player) -> PlaybackState {
    match player.get_playback_status() {
        Ok(PlaybackStatus::Playing) => PlaybackState::Playing,
        Ok(PlaybackStatus::Paused) => PlaybackState::Paused,
        Ok(PlaybackStatus::Stopped) => PlaybackState::Stopped,
        Err(_) => PlaybackState::Unknown,
    }
}

fn artwork_source(value: &str) -> Option<ArtworkSource> {
    if !has_valid_percent_encoding(value) {
        return None;
    }

    let url = Url::parse(value).ok()?;
    match url.scheme() {
        "file" => url.to_file_path().ok().map(ArtworkSource::Local),
        "http" | "https" => Some(ArtworkSource::Remote(url.into())),
        _ => None,
    }
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
    use super::{
        MPRIS_PLAYER_INTERFACE, artwork_source, is_mpris_owner_change,
        is_relevant_properties_change, snapshot_has_changed,
    };
    use crate::model::{ArtworkSource, MediaSnapshot};
    use dbus::arg::{PropMap, Variant};
    use std::path::PathBuf;

    fn changed_properties(names: &[&str]) -> PropMap {
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    Variant(Box::new(String::new()) as Box<dyn dbus::arg::RefArg>),
                )
            })
            .collect()
    }

    #[test]
    fn refreshes_for_displayed_player_properties() {
        for property in ["Metadata", "PlaybackStatus"] {
            assert!(is_relevant_properties_change(
                MPRIS_PLAYER_INTERFACE,
                &changed_properties(&[property]),
                &[],
            ));
            assert!(is_relevant_properties_change(
                MPRIS_PLAYER_INTERFACE,
                &PropMap::new(),
                &[property.to_owned()],
            ));
        }
    }

    #[test]
    fn ignores_unrelated_property_changes() {
        assert!(!is_relevant_properties_change(
            MPRIS_PLAYER_INTERFACE,
            &changed_properties(&["Volume", "Shuffle", "Position"]),
            &[],
        ));
        assert!(!is_relevant_properties_change(
            "org.mpris.MediaPlayer2",
            &changed_properties(&["Metadata"]),
            &[],
        ));
    }

    #[test]
    fn refreshes_only_for_mpris_player_owner_changes() {
        assert!(is_mpris_owner_change(
            "org.mpris.MediaPlayer2.spotify",
            "",
            ":1.42",
        ));
        assert!(is_mpris_owner_change(
            "org.mpris.MediaPlayer2.spotify",
            ":1.42",
            "",
        ));
        assert!(is_mpris_owner_change(
            "org.mpris.MediaPlayer2.spotify",
            ":1.42",
            ":1.43",
        ));
        assert!(!is_mpris_owner_change(
            "org.example.Application",
            "",
            ":1.42",
        ));
        assert!(!is_mpris_owner_change(
            "org.mpris.MediaPlayer2.spotify",
            ":1.42",
            ":1.42",
        ));
    }

    #[test]
    fn suppresses_identical_snapshots() {
        let snapshot = MediaSnapshot::default();

        assert!(snapshot_has_changed(&None, &snapshot));
        assert!(!snapshot_has_changed(&Some(snapshot.clone()), &snapshot));
    }

    #[test]
    fn parses_local_file_urls() {
        assert_eq!(
            artwork_source("file:///home/user/Music/Album%20Art.png"),
            Some(ArtworkSource::Local(PathBuf::from(
                "/home/user/Music/Album Art.png"
            )))
        );
        assert_eq!(
            artwork_source("file://localhost/home/user/M%C3%BAsica.png"),
            Some(ArtworkSource::Local(PathBuf::from(
                "/home/user/Música.png"
            )))
        );
    }

    #[test]
    fn parses_unescaped_unicode_file_urls() {
        assert_eq!(
            artwork_source("file:///home/user/Música/Portada.png"),
            Some(ArtworkSource::Local(PathBuf::from(
                "/home/user/Música/Portada.png"
            )))
        );
    }

    #[test]
    fn rejects_non_local_and_malformed_file_urls() {
        assert_eq!(
            artwork_source("file://example.com/home/user/cover.png"),
            None
        );
        assert_eq!(artwork_source("file:///home/user/bad%2.png"), None);
        assert_eq!(artwork_source("file:///home/user/bad%ZZ.png"), None);
    }

    #[test]
    fn parses_supported_artwork_sources() {
        assert_eq!(
            artwork_source("file:///home/user/Music/Album%20Art.png"),
            Some(ArtworkSource::Local(PathBuf::from(
                "/home/user/Music/Album Art.png"
            )))
        );
        assert_eq!(
            artwork_source("https://example.com/cover%20art.png"),
            Some(ArtworkSource::Remote(
                "https://example.com/cover%20art.png".to_owned()
            ))
        );
        assert_eq!(
            artwork_source("http://example.com/cover.png"),
            Some(ArtworkSource::Remote(
                "http://example.com/cover.png".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_unsupported_and_malformed_artwork_sources() {
        assert_eq!(artwork_source("ftp://example.com/cover.png"), None);
        assert_eq!(artwork_source("https://example.com/bad%ZZ.png"), None);
        assert_eq!(artwork_source("not a URL"), None);
    }
}
