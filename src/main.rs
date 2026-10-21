mod album_art;
mod album_color;
mod i18n;
mod media;
mod model;
mod settings;
mod style;
mod ui;
mod window;

use crate::window::Window;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    cosmic::applet::run::<Window>(())?;

    Ok(())
}
