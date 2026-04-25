mod control;
mod danmaku;
pub mod video;
mod error;

pub use video::*;

pub fn control_init() {
    control::init();
}
