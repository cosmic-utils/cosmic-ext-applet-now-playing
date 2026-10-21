use std::thread;

use cosmic::app::Core;
use cosmic::iced::futures::channel::oneshot;
use cosmic::iced::{Limits, Subscription, window, window::Id};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::{Action, Element, Task};

use crate::album_art::{self, LoadedArt};
use crate::media::{self, MediaCommand};
use crate::model::{AppModel, ArtKey, MediaSnapshot, PopupPage};
use crate::{settings, ui};

const ID: &str = "com.github.DiegoMMR.CosmicExtAppletNowPlaying";

pub struct Window {
    core: Core,
    popup: Option<Id>,
    model: AppModel,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    ShowMain,
    ShowPlayerList,
    ShowSettings,
    SetAlbumColorEnabled(bool),
    SetPreviousControlEnabled(bool),
    SetPlayControlEnabled(bool),
    SetNextControlEnabled(bool),
    SelectPlayer(String),
    PopupClosed(Id),
    MediaChanged(MediaSnapshot),
    AlbumArtLoaded(ArtKey, Result<LoadedArt, String>),
    PreviousTrack,
    TogglePlayPause,
    NextTrack,
}

impl cosmic::Application for Window {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let model = AppModel::new(settings::load(), media::initial_snapshot());
        let mut window = Self {
            core,
            popup: None,
            model,
        };
        let artwork = window.artwork_task();
        (window, artwork)
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Action<Self::Message>> {
        // Any message may change the current track, so check the cover after each one.
        let task = self.handle_message(message);
        Task::batch([task, self.artwork_task()])
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        media::subscription().map(Message::MediaChanged)
    }

    fn view(&self) -> Element<'_, Message> {
        ui::panel(&self.core, &self.model)
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        ui::popup(&self.core, &self.model)
    }
}

impl Window {
    fn handle_message(&mut self, message: Message) -> Task<Action<Message>> {
        match message {
            Message::TogglePopup => return self.toggle_popup(),
            Message::ShowMain => self.model.set_popup_page(PopupPage::Main),
            Message::ShowPlayerList => self.model.set_popup_page(PopupPage::PlayerList),
            Message::ShowSettings => self.model.set_popup_page(PopupPage::Settings),
            Message::SetAlbumColorEnabled(enabled) => {
                self.model.set_album_color_enabled(enabled);
                self.save_settings();
            }
            Message::SetPreviousControlEnabled(enabled) => {
                self.model.set_previous_control_enabled(enabled);
                self.save_settings();
            }
            Message::SetPlayControlEnabled(enabled) => {
                self.model.set_play_control_enabled(enabled);
                self.save_settings();
            }
            Message::SetNextControlEnabled(enabled) => {
                self.model.set_next_control_enabled(enabled);
                self.save_settings();
            }
            Message::SelectPlayer(id) => {
                self.model.select_player(id);
                self.model.set_popup_page(PopupPage::Main);
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                    self.model.set_popup_page(PopupPage::Main);
                }
            }
            Message::MediaChanged(snapshot) => {
                self.model.apply_snapshot(snapshot);
                if !self.model.has_active_media()
                    && let Some(popup) = self.popup.take()
                {
                    self.model.set_popup_page(PopupPage::Main);
                    return surface_task(destroy_popup(popup));
                }
            }
            Message::AlbumArtLoaded(key, result) => {
                if let Err(error) = &result {
                    eprintln!(
                        "unable to load album art from {}: {error}",
                        album_art::redact(&key.url)
                    );
                }
                self.model.art_loaded(key, result);
            }
            Message::PreviousTrack => self.run_media_command(MediaCommand::Previous),
            Message::TogglePlayPause => {
                self.run_media_command(MediaCommand::TogglePlayPause);
            }
            Message::NextTrack => self.run_media_command(MediaCommand::Next),
        }

        Task::none()
    }

    /// Loads the current cover on a separate thread, so slow disks or
    /// networks never block the panel.
    fn artwork_task(&mut self) -> Task<Action<Message>> {
        let Some(key) = self.model.take_artwork_request() else {
            return Task::none();
        };

        cosmic::task::future(async move {
            let (sender, receiver) = oneshot::channel();
            let url = key.url.clone();
            let spawned = thread::Builder::new()
                .name("album-art".to_owned())
                .spawn(move || {
                    let _ = sender.send(album_art::load(&url));
                });

            // Always answer, so the cover is never left pending.
            let result = match spawned {
                Ok(_) => receiver
                    .await
                    .unwrap_or_else(|_| Err("album art loader stopped unexpectedly".to_owned())),
                Err(error) => Err(format!("unable to start album art loader: {error}")),
            };
            Message::AlbumArtLoaded(key, result)
        })
    }

    fn toggle_popup(&mut self) -> Task<Action<Message>> {
        if let Some(popup) = self.popup.take() {
            self.model.set_popup_page(PopupPage::Main);
            return surface_task(destroy_popup(popup));
        }

        let Some(parent) = self.core.main_window_id() else {
            eprintln!("unable to open popup: main applet window is unavailable");
            return Task::none();
        };

        surface_task(app_popup::<Window>(
            |_| Default::default(),
            move |state: &mut Window| {
                let popup = Id::unique();
                let mut popup_settings =
                    state
                        .core
                        .applet
                        .get_popup_settings(parent, popup, None, None, None);
                popup_settings.positioner.size_limits = Limits::NONE
                    .max_width(370.0)
                    .min_width(200.0)
                    .min_height(200.0)
                    .max_height(1080.0);
                state.popup = Some(popup);

                popup_settings
            },
            Some(Box::new(|state: &Window| {
                ui::popup(&state.core, &state.model).map(cosmic::Action::App)
            })),
        ))
    }

    fn run_media_command(&self, command: MediaCommand) {
        if let Err(error) = media::run_command(self.model.selected_player.as_deref(), command) {
            eprintln!("{error}");
        }
    }

    fn save_settings(&self) {
        if let Err(error) = settings::save(self.model.settings) {
            eprintln!("unable to save settings: {error}");
        }
    }
}

fn surface_task(action: cosmic::surface::Action) -> Task<Action<Message>> {
    cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
        action,
    )))
}
