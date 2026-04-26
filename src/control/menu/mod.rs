mod menu;

use glib::prelude::*;
use menu::*;

pub fn init() {
    MenuActions::ensure_type();

    gtk::gio::resources_register_include!("mutsumi.gresource")
        .expect("Failed to register resources.");
}
