mod format;
mod menu;
mod player;
mod scale;
mod sidebar;
mod toast;
mod volume_bar;

pub use format::*;
pub use menu::*;
pub use player::*;
pub use scale::*;
pub use sidebar::*;
pub use toast::*;
pub use volume_bar::*;

use gtk::prelude::*;

pub fn init() {
    gtk::gio::resources_register_include!("mutsumi.gresource")
        .expect("Failed to register resources.");

    PlayerPage::ensure_type();
    ControlSidebar::ensure_type();
    MenuActions::ensure_type();
    VideoScale::ensure_type();
    VolumeBar::ensure_type();

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_resource_path("/io/github/mutsumi/icons");

        let provider = gtk::CssProvider::new();
        provider.load_from_string(CONTROL_CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

const CONTROL_CSS: &str = "
.mpv-top-bar {
  background-image: linear-gradient(to bottom,
      rgba(0, 0, 0, 0.72),
      rgba(0, 0, 0, 0.34) 55%,
      rgba(0, 0, 0, 0));
  border-radius: 0;
  color: white;
  padding: 18px 20px 42px;
}

.mpv-bottom-bar {
  background-image: linear-gradient(to top,
      rgba(0, 0, 0, 0.78),
      rgba(0, 0, 0, 0.48) 58%,
      rgba(0, 0, 0, 0));
  border-radius: 0;
  color: white;
  padding: 48px 36px 24px;
}

.mpv-play-button {
  min-height: 44px;
  min-width: 44px;
}

.mpv-seekbar > trough {
  min-height: 5px;
}

.volume-bar {
  padding: 10px;
  border-radius: 50px;
}

.speed-label {
  text-shadow: 3px 3px 5px alpha(black, 1);
}
";
