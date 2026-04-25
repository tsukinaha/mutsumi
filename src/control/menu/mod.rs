mod menu;

use menu::*;
use glib::prelude::*;

pub fn init() {
    MenuActions::ensure_type();

    gtk::gio::resources_register_include!("mutsumi.gresource")
        .expect("Failed to register resources.");
}
