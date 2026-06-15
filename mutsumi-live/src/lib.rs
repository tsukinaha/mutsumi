mod danmaku;
mod playlist;
mod window;

use gtk::prelude::*;

pub use playlist::*;
pub use window::*;

pub fn init() {
    gtk::gio::resources_register_include!("mutsumi-live.gresource")
        .expect("Failed to register resources.");

    SourceActionRow::ensure_type();
    PlayList::ensure_type();
    MutsumiLiveWindow::ensure_type();
}
