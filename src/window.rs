use std::path::PathBuf;
use std::time::Duration;

use cosmic::app::Core;
use cosmic::iced::{
    platform_specific::shell::commands::popup::{destroy_popup, get_popup},
    stream::channel,
    window::Id,
    Background,
    Color,
    ContentFit,
    Length,
    Limits,
    Subscription,
};
use cosmic::iced_core::text::{Ellipsize, EllipsizeHeightLimit, Wrapping};
use cosmic::iced_runtime::core::window;
use cosmic::widget::{button, button::Catalog, column, icon, text, Row};
use cosmic::{Action, Element, Task};
use mpris::{Event as MprisEvent, PlayerFinder};

use crate::album_color::dominant_album_color;
use crate::fl;
use crate::metadata::{now_playing_from_player, now_playing_snapshot, NowPlayingData};
use crate::player::{album_art_path_from_metadata, playback_state_from_player, with_active_player};

const ID: &str = "com.github.DiegoMMR.CosmicExtAppletNowPlaying";

#[derive(Default)]
pub struct Window {
    core: Core,
    popup: Option<Id>,
    now_playing_text: String,
    now_playing_title: String,
    now_playing_artist: String,
    playback_state: PlaybackState,
    album_art_path: Option<PathBuf>,
    album_color: Option<Color>,
    has_active_media: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    #[default]
    Unknown,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    NowPlayingChanged(NowPlayingData),
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
        let initial = now_playing_snapshot();

        let window = Window {
            core,
            now_playing_text: initial.text,
            now_playing_title: initial.title,
            now_playing_artist: initial.artist,
            playback_state: initial.state,
            album_color: dominant_album_color(initial.album_art_path.as_deref()),
            album_art_path: initial.album_art_path,
            has_active_media: initial.has_active_media,
            ..Default::default()
        };

        (window, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                return if let Some(popup_id) = self.popup.take() {
                    destroy_popup(popup_id)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);

                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );

                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(370.0)
                        .min_width(200.0)
                        .min_height(200.0)
                        .max_height(1080.0);

                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(popup_id) => {
                if self.popup.as_ref() == Some(&popup_id) {
                    self.popup = None;
                }
            }
            Message::NowPlayingChanged(data) => {
                self.now_playing_text = data.text;
                self.now_playing_title = data.title;
                self.now_playing_artist = data.artist;
                self.playback_state = data.state;
                self.album_color = dominant_album_color(data.album_art_path.as_deref());
                self.album_art_path = data.album_art_path;
                self.has_active_media = data.has_active_media;
            }
            Message::PreviousTrack => {
                with_active_player(|player| {
                    let _ = player.previous();
                });
            }
            Message::TogglePlayPause => {
                with_active_player(|player| {
                    let _ = player.play_pause();
                });
            }
            Message::NextTrack => {
                with_active_player(|player| {
                    let _ = player.next();
                });
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::run(|| {
            channel(
                64,
                |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                    std::thread::spawn(move || {
                        let mut last_sent = String::new();
                        let mut last_state = PlaybackState::Unknown;
                        let mut last_art: Option<PathBuf> = None;

                        loop {
                            let finder = match PlayerFinder::new() {
                                Ok(finder) => finder,
                                Err(_) => {
                                    std::thread::sleep(Duration::from_millis(1000));
                                    continue;
                                }
                            };

                            let player = match finder.find_active() {
                                Ok(player) => player,
                                Err(_) => {
                                    if last_sent != fl!("nothing-playing")
                                        || last_state != PlaybackState::Stopped
                                    {
                                        last_sent = fl!("nothing-playing");
                                        last_state = PlaybackState::Stopped;
                                        last_art = None;
                                        while output
                                            .try_send(Message::NowPlayingChanged(NowPlayingData {
                                                text: last_sent.clone(),
                                                title: fl!("nothing-playing"),
                                                artist: String::new(),
                                                state: last_state,
                                                album_art_path: None,
                                                has_active_media: false,
                                            }))
                                            .is_err()
                                        {
                                            std::thread::sleep(Duration::from_millis(10));
                                        }
                                    }

                                    std::thread::sleep(Duration::from_millis(1000));
                                    continue;
                                }
                            };

                            let current = now_playing_from_player(&player);
                            let current_state = current.state;
                            let current_art = current.album_art_path.clone();
                            if current.text != last_sent
                                || current_state != last_state
                                || current_art != last_art
                            {
                                last_sent = current.text.clone();
                                last_state = current_state;
                                last_art = current_art.clone();
                                while output
                                    .try_send(Message::NowPlayingChanged(current.clone()))
                                    .is_err()
                                {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                            }

                            let mut events = match player.events() {
                                Ok(events) => events,
                                Err(_) => {
                                    std::thread::sleep(Duration::from_millis(300));
                                    continue;
                                }
                            };

                            for event in &mut events {
                                match event {
                                    Ok(MprisEvent::TrackChanged(metadata)) => {
                                        let title = metadata
                                            .title()
                                            .map(ToOwned::to_owned)
                                            .unwrap_or_else(|| fl!("unknown-title"));
                                        let artist = metadata
                                            .artists()
                                            .and_then(|a| a.first().copied())
                                            .map(ToOwned::to_owned)
                                            .unwrap_or_else(|| fl!("unknown-artist"));
                                        let text = format!("{} - {}", title, artist);
                                        let state = playback_state_from_player(&player);
                                        let art = album_art_path_from_metadata(&metadata);

                                        if text != last_sent || state != last_state || art != last_art {
                                            last_sent = text.clone();
                                            last_state = state;
                                            last_art = art.clone();
                                            while output
                                                .try_send(Message::NowPlayingChanged(
                                                    NowPlayingData {
                                                        text: text.clone(),
                                                        title: title.clone(),
                                                        artist: artist.clone(),
                                                        state,
                                                        album_art_path: art.clone(),
                                                        has_active_media: true,
                                                    },
                                                ))
                                                .is_err()
                                            {
                                                std::thread::sleep(Duration::from_millis(10));
                                            }
                                        }
                                    }
                                    Ok(MprisEvent::Playing)
                                    | Ok(MprisEvent::Paused)
                                    | Ok(MprisEvent::Stopped) => {
                                        let data = now_playing_from_player(&player);
                                        let text = data.text.clone();
                                        let state = data.state;
                                        let art = data.album_art_path.clone();

                                        if text != last_sent || state != last_state || art != last_art {
                                            last_sent = text;
                                            last_state = state;
                                            last_art = art.clone();
                                            while output
                                                .try_send(Message::NowPlayingChanged(data.clone()))
                                                .is_err()
                                            {
                                                std::thread::sleep(Duration::from_millis(10));
                                            }
                                        }
                                    }
                                    Ok(MprisEvent::PlayerShutDown) | Err(_) => break,
                                    _ => {}
                                }
                            }

                            std::thread::sleep(Duration::from_millis(200));
                        }
                    });
                },
            )
        })
    }

    fn view(&self) -> Element<'_, Message> {
        if !self.has_active_media() {
            return self.core.applet.autosize_window(text("")).into();
        }

        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);
        let transport_icon = match self.playback_state {
            PlaybackState::Playing => "media-playback-pause-symbolic",
            PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Unknown => {
                "media-playback-start-symbolic"
            }
        };

        let row_content = Row::new()
            .spacing(pad.0)
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(icon::from_name(transport_icon).size(size.0))
            .push(
                text(self.now_playing_text.as_str())
                    .size(size.0.saturating_sub(1))
                    .width(Length::Fixed(260.0))
                    .wrapping(Wrapping::None)
                    .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
            );

        let album_color = self.album_color;
        let content = button::custom(row_content)
            .width(Length::Shrink)
            .height(Length::Shrink)
            .class(cosmic::theme::Button::Custom {
                active: Box::new(move |focused, theme| {
                    let base = theme.active(focused, false, &cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color)
                }),
                disabled: Box::new(move |theme| {
                    let base = theme.disabled(&cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color)
                }),
                hovered: Box::new(move |focused, theme| {
                    let base = theme.hovered(focused, false, &cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color.map(|c| shift_color(c, 0.07)))
                }),
                pressed: Box::new(move |focused, theme| {
                    let base = theme.pressed(focused, false, &cosmic::theme::Button::AppletIcon);
                    style_with_optional_album_color(base, album_color.map(|c| shift_color(c, -0.08)))
                }),
            })
            .on_press(Message::TogglePopup);

        self.core.applet.autosize_window(content).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        if !self.has_active_media() {
            return self.core.applet.popup_container(text("")).into();
        }

        let size = self.core.applet.suggested_size(true);
        let pad = self.core.applet.suggested_padding(true);
        let transport_icon = match self.playback_state {
            PlaybackState::Playing => "media-playback-pause-symbolic",
            PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Unknown => {
                "media-playback-start-symbolic"
            }
        };
        let album_height = size.0.saturating_mul(4);
        let album_width = album_height.saturating_mul(16) / 9;

        let album_widget = self
            .album_art_path
            .as_ref()
            .map(|path| {
                icon::icon(icon::from_path(path.clone()))
                    .height(Length::Fixed(f32::from(album_height)))
                    .width(Length::Fixed(f32::from(album_width)))
                    .content_fit(ContentFit::Contain)
            })
            .unwrap_or_else(|| {
                icon::from_name("audio-x-generic-symbolic")
                    .size(album_height)
                    .icon()
                    .height(Length::Fixed(f32::from(album_height)))
                    .width(Length::Fixed(f32::from(album_width)))
                    .content_fit(ContentFit::Contain)
            });

        let controls = Row::new()
            .spacing(pad.0)
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .push(
                button::icon(icon::from_name("media-skip-backward-symbolic").size(size.0 + 4))
                    .on_press(Message::PreviousTrack),
            )
            .push(
                button::icon(icon::from_name(transport_icon).size(size.0 + 4))
                    .on_press(Message::TogglePlayPause),
            )
            .push(
                button::icon(icon::from_name("media-skip-forward-symbolic").size(size.0 + 4))
                    .on_press(Message::NextTrack),
            );

        let media_info = column()
            .spacing(2)
            .align_x(cosmic::iced::Alignment::Center)
            .push(
                text(self.now_playing_title.as_str())
                    .size(size.0.saturating_sub(1))
                    .width(Length::Fill)
                    .align_x(cosmic::iced::alignment::Horizontal::Center)
                    .wrapping(Wrapping::WordOrGlyph),
            )
            .push(
                text(self.now_playing_artist.as_str())
                    .size(size.0.saturating_sub(3))
                    .width(Length::Fill)
                    .align_x(cosmic::iced::alignment::Horizontal::Center)
                    .wrapping(Wrapping::WordOrGlyph),
            );

        let content_list = column()
            .padding(12)
            .spacing(12)
            .align_x(cosmic::iced::Alignment::Center)
            .push(album_widget)
            .push(media_info)
            .push(controls);

        self.core.applet.popup_container(content_list).into()
    }
}

impl Window {
    fn has_active_media(&self) -> bool {
        self.has_active_media
    }
}

fn style_with_optional_album_color(mut base: button::Style, color: Option<Color>) -> button::Style {
    if let Some(album) = color {
        let theme_base = match base.background {
            Some(Background::Color(c)) => c,
            _ => Color::from_rgb8(36, 38, 42),
        };

        let mixed = blend_color(theme_base, album, 0.36);
        let background = with_alpha(mixed, 0.64);
        let border = with_alpha(shift_color(album, -0.05), 0.82);

        let foreground = contrast_text_color(background);
        base.background = Some(Background::Color(background));
        base.border_width = base.border_width.max(1.0);
        base.border_color = border;
        base.text_color = Some(foreground);
        base.icon_color = Some(foreground);
    }
    base
}

fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.a = alpha.clamp(0.0, 1.0);
    color
}

fn blend_color(a: Color, b: Color, ratio: f32) -> Color {
    let t = ratio.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Color {
        r: (a.r * inv) + (b.r * t),
        g: (a.g * inv) + (b.g * t),
        b: (a.b * inv) + (b.b * t),
        a: (a.a * inv) + (b.a * t),
    }
}

fn contrast_text_color(background: Color) -> Color {
    let luminance = 0.2126 * background.r + 0.7152 * background.g + 0.0722 * background.b;
    if luminance > 0.58 {
        Color::from_rgb8(17, 17, 17)
    } else {
        Color::WHITE
    }
}

fn shift_color(color: Color, amount: f32) -> Color {
    let adjust = |channel: f32| (channel + amount).clamp(0.0, 1.0);
    Color {
        r: adjust(color.r),
        g: adjust(color.g),
        b: adjust(color.b),
        a: color.a,
    }
}
