// mod sidebar;
pub mod menu;
pub mod overlay;
pub mod player;
pub mod toast;
//pub mod scale;
pub mod volume_bar;

pub use toast::*;

pub fn init() {
    menu::init();
}
