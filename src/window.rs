use cosmic::app::Core;
use cosmic::iced::{Limits, Subscription, window, window::Id};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::{Action, Element, Task};

use crate::media::{self, MediaCommand};
use crate::model::{AppModel, MediaSnapshot, PopupPage};
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
        (
            Self {
                core,
                popup: None,
                model,
            },
            Task::none(),
        )
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Action<Self::Message>> {
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
            Message::PreviousTrack => self.run_media_command(MediaCommand::Previous),
            Message::TogglePlayPause => {
                self.run_media_command(MediaCommand::TogglePlayPause);
            }
            Message::NextTrack => self.run_media_command(MediaCommand::Next),
        }

        Task::none()
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
