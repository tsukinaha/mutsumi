mod control;
mod danmaku;
mod error;
pub mod video;

pub use video::*;

pub fn control_init() {
    control::init();
}
