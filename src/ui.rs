use cosmic::Element;
use cosmic::app::Core;
use cosmic::iced::advanced::text::{Ellipsize, EllipsizeHeightLimit, Wrapping};
use cosmic::iced::{ContentFit, Length};
use cosmic::widget::{
    Column, Row, button, button::Catalog, icon, mouse_area, settings, text, toggler,
};

use crate::fl;
use crate::model::{AppModel, PlaybackState, PopupPage};
use crate::style::{applet_button, shift_color};
use crate::window::Message;

pub fn panel<'a>(core: &Core, model: &'a AppModel) -> Element<'a, Message> {
    if !model.has_active_media() {
        return core.applet.autosize_window(text("")).into();
    }

    let size = core.applet.suggested_size(true);
    let padding = core.applet.suggested_padding(true);
    let mut content = Row::new()
        .spacing(padding.0)
        .padding([0, padding.0])
        .align_y(cosmic::iced::alignment::Vertical::Center);

    if model.settings.previous_control_enabled {
        content = content.push(
            mouse_area(icon::from_name("media-skip-backward-symbolic").size(size.0))
                .on_press(Message::PreviousTrack),
        );
    }
    if model.settings.play_control_enabled {
        content = content.push(
            mouse_area(icon::from_name(transport_icon(model)).size(size.0))
                .on_press(Message::TogglePlayPause),
        );
    }
    if model.settings.next_control_enabled {
        content = content.push(
            mouse_area(icon::from_name("media-skip-forward-symbolic").size(size.0))
                .on_press(Message::NextTrack),
        );
    }

    content = content.push(
        text(model.now_playing.text.as_str())
            .size(size.0.saturating_sub(1))
            .width(Length::Fixed(260.0))
            .wrapping(Wrapping::None)
            .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1))),
    );

    let album_color = model.album_color;
    let button = button::custom(content)
        .width(Length::Shrink)
        .height(Length::Shrink)
        .name(fl!("now-playing"))
        .class(cosmic::theme::Button::Custom {
            active: Box::new(move |focused, theme| {
                let base = theme.active(focused, false, &cosmic::theme::Button::AppletIcon);
                applet_button(base, album_color)
            }),
            disabled: Box::new(move |theme| {
                let base = theme.disabled(&cosmic::theme::Button::AppletIcon);
                applet_button(base, album_color)
            }),
            hovered: Box::new(move |focused, theme| {
                let base = theme.hovered(focused, false, &cosmic::theme::Button::AppletIcon);
                applet_button(base, album_color.map(|color| shift_color(color, 0.07)))
            }),
            pressed: Box::new(move |focused, theme| {
                let base = theme.pressed(focused, false, &cosmic::theme::Button::AppletIcon);
                applet_button(base, album_color.map(|color| shift_color(color, -0.08)))
            }),
        })
        .on_press(Message::TogglePopup);

    core.applet.autosize_window(button).into()
}

pub fn popup<'a>(core: &Core, model: &'a AppModel) -> Element<'a, Message> {
    if !model.has_active_media() {
        return core.applet.popup_container(text("")).into();
    }

    let content = match model.popup_page {
        PopupPage::Main => main_popup(core, model),
        PopupPage::PlayerList => player_list(core, model),
        PopupPage::Settings => settings_page(core, model),
    };

    core.applet.popup_container(content).into()
}

fn main_popup<'a>(core: &Core, model: &'a AppModel) -> Element<'a, Message> {
    let size = core.applet.suggested_size(true);
    let padding = core.applet.suggested_padding(true);
    let artwork_size = size.0.saturating_mul(4);

    let player_selector: Element<'_, Message> = if model.players.len() > 1 {
        accessible_icon_button_without_tooltip(
            "view-list-symbolic",
            size.0,
            fl!("select-player"),
            Message::ShowPlayerList,
        )
    } else {
        text("").width(Length::Fixed(f32::from(size.0))).into()
    };

    let header = Row::new()
        .width(Length::Fill)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .push(player_selector)
        .push(text("").width(Length::Fill))
        .push(accessible_icon_button_without_tooltip(
            "preferences-system-symbolic",
            size.0,
            fl!("settings"),
            Message::ShowSettings,
        ));

    let artwork = model.now_playing.album_art_path.as_ref().map_or_else(
        || {
            icon::from_name("audio-x-generic-symbolic")
                .size(artwork_size)
                .icon()
                .height(Length::Fixed(f32::from(artwork_size)))
                .width(Length::Fixed(f32::from(artwork_size)))
                .content_fit(ContentFit::Contain)
        },
        |path| {
            icon::icon(icon::from_path(path.clone()))
                .height(Length::Fixed(f32::from(artwork_size)))
                .width(Length::Fixed(f32::from(artwork_size)))
                .content_fit(ContentFit::Contain)
        },
    );

    let media_info = Column::new()
        .spacing(4)
        .push(
            text(model.now_playing.title.as_str())
                .size(size.0.saturating_sub(1))
                .width(Length::Fill)
                .wrapping(Wrapping::WordOrGlyph),
        )
        .push(
            text(model.now_playing.artist.as_str())
                .size(size.0.saturating_sub(3))
                .width(Length::Fill)
                .wrapping(Wrapping::WordOrGlyph),
        );

    let media_row = Row::new()
        .spacing(12)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .push(artwork)
        .push(media_info.width(Length::Fill));

    let controls = Row::new()
        .spacing(padding.0)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .push(accessible_icon_button_without_tooltip(
            "media-skip-backward-symbolic",
            size.0 + 4,
            fl!("previous-track"),
            Message::PreviousTrack,
        ))
        .push(accessible_icon_button_without_tooltip(
            transport_icon(model),
            size.0 + 4,
            fl!("toggle-play-pause"),
            Message::TogglePlayPause,
        ))
        .push(accessible_icon_button_without_tooltip(
            "media-skip-forward-symbolic",
            size.0 + 4,
            fl!("next-track"),
            Message::NextTrack,
        ));

    Column::new()
        .padding(12)
        .spacing(12)
        .align_x(cosmic::iced::Alignment::Center)
        .push(header)
        .push(media_row)
        .push(controls)
        .into()
}

fn player_list<'a>(core: &Core, model: &'a AppModel) -> Element<'a, Message> {
    let size = core.applet.suggested_size(true);
    let header = page_header(size.0, fl!("select-player"));
    let list =
        model
            .players
            .iter()
            .enumerate()
            .fold(Column::new().spacing(4), |list, (index, player)| {
                let selected = model.selected_player.as_deref() == Some(player.id.as_str());
                let marker: Element<'_, Message> = if selected {
                    icon::from_name("emblem-ok-symbolic")
                        .size(size.0.saturating_sub(2))
                        .into()
                } else {
                    text("").width(Length::Fixed(f32::from(size.0))).into()
                };
                let list = if index > 0 {
                    list.push(
                        cosmic::widget::divider::horizontal::light()
                            .height(1.0)
                            .width(Length::Fill),
                    )
                } else {
                    list
                };

                list.push(
                    button::custom(
                        Row::new()
                            .spacing(8)
                            .align_y(cosmic::iced::alignment::Vertical::Center)
                            .push(marker)
                            .push(
                                Column::new()
                                    .spacing(1)
                                    .push(text(player.identity.as_str()))
                                    .push(
                                        text(player.track())
                                            .size(size.0.saturating_sub(3))
                                            .wrapping(Wrapping::WordOrGlyph),
                                    ),
                            ),
                    )
                    .width(Length::Fill)
                    .name(format!("{}: {}", fl!("select-player"), player.identity))
                    .selected(selected)
                    .class(cosmic::theme::Button::ListItem([4.0; 4]))
                    .on_press(Message::SelectPlayer(player.id.clone())),
                )
            });

    Column::new()
        .spacing(12)
        .padding(12)
        .push(header)
        .push(list)
        .into()
}

fn settings_page<'a>(core: &Core, model: &'a AppModel) -> Element<'a, Message> {
    let size = core.applet.suggested_size(true);
    Column::new()
        .spacing(12)
        .padding(12)
        .push(page_header(size.0, fl!("settings")))
        .push(settings::item(
            fl!("use-album-colors"),
            toggler(model.settings.album_color_enabled).on_toggle(Message::SetAlbumColorEnabled),
        ))
        .push(settings::item(
            fl!("previous-track"),
            toggler(model.settings.previous_control_enabled)
                .on_toggle(Message::SetPreviousControlEnabled),
        ))
        .push(settings::item(
            fl!("toggle-play-pause"),
            toggler(model.settings.play_control_enabled)
                .on_toggle(Message::SetPlayControlEnabled),
        ))
        .push(settings::item(
            fl!("next-track"),
            toggler(model.settings.next_control_enabled)
                .on_toggle(Message::SetNextControlEnabled),
        ))
        .into()
}

fn page_header(size: u16, title: String) -> Element<'static, Message> {
    Row::new()
        .width(Length::Fill)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .push(accessible_icon_button_without_tooltip(
            "go-previous-symbolic",
            size,
            fl!("back"),
            Message::ShowMain,
        ))
        .push(text(title).size(size))
        .push(text("").width(Length::Fill))
        .into()
}

fn accessible_icon_button_without_tooltip(
    icon_name: &str,
    size: u16,
    label: String,
    message: Message,
) -> Element<'static, Message> {
    button::icon(icon::from_name(icon_name.to_owned()).size(size))
        .name(label)
        .on_press(message)
        .into()
}

fn transport_icon(model: &AppModel) -> &'static str {
    match model.now_playing.state {
        PlaybackState::Playing => "media-playback-pause-symbolic",
        PlaybackState::Paused | PlaybackState::Stopped | PlaybackState::Unknown => {
            "media-playback-start-symbolic"
        }
    }
}
