mod core;
mod metadata;
mod player;
mod window;

use crate::window::Window;

fn main() -> cosmic::iced::Result {
    // Trigger localization initialization before UI starts.
    let _ = &*core::localization::LANGUAGE_LOADER;
    cosmic::applet::run::<Window>(())?;

    Ok(())
}
