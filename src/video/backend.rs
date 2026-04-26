use std::{future::Future, pin::Pin};

use gtk::gdk::ModifierType;

pub type BoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSelection {
    Track(i64),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

impl std::fmt::Display for TrackSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            TrackSelection::Track(id) => id.to_string(),
            TrackSelection::None => "no".to_string(),
        };
        write!(f, "{str}")
    }
}

pub trait VideoBackend {
    fn name() -> &'static str
    where
        Self: Sized;

    fn play(&self, url: &str, percentage: f64);
    fn shutdown(&self);
    fn stop(&self);

    fn load_video(&self, url: &str);
    fn add_sub(&self, url: &str);

    fn pause(&self, pause: bool);
    fn command_pause(&self);

    fn set_position(&self, value: f64);
    fn set_percent_position(&self, value: f64);
    fn set_start(&self, percentage: f64);

    fn set_volume(&self, value: i64);
    fn volume_scroll(&self, value: i64);
    fn set_speed(&self, value: f64);

    fn seek_forward(&self, value: i64);
    fn seek_backward(&self, value: i64);

    fn position(&self) -> BoxedFuture<'_, f64>;
    fn paused(&self) -> BoxedFuture<'_, bool>;

    fn duration(&self) -> BoxedFuture<'_, f64> {
        Box::pin(async { 0.0 })
    }

    fn seek_relative(&self, value: i64) {
        if value >= 0 {
            self.seek_forward(value);
        } else {
            self.seek_backward(-value);
        }
    }

    fn set_aid(&self, _value: TrackSelection) {}

    fn set_sid(&self, _value: TrackSelection) {}

    fn disable_aid(&self) {
        self.set_aid(TrackSelection::None);
    }

    fn disable_sid(&self) {
        self.set_sid(TrackSelection::None);
    }

    fn set_keep_aspect_ratio(&self, _keep: bool) {}

    fn set_slang(&self, _value: String) {}

    fn get_track_id(&self, _kind: TrackKind) -> BoxedFuture<'_, i64>;

    fn press_key(&self, _key: u32, _state: ModifierType) {}

    fn release_key(&self, _key: u32, _state: ModifierType) {}

    fn display_stats_toggle(&self) {}

    fn set_brightness(&self, _value: f64) {}

    fn set_contrast(&self, _value: f64) {}

    fn set_gamma(&self, _value: f64) {}

    fn set_hue(&self, _value: f64) {}

    fn set_saturation(&self, _value: f64) {}

    fn set_sub_pos(&self, _value: f64) {}

    fn set_sub_font_size(&self, _value: f64) {}

    fn set_sub_scale(&self, _value: f64) {}

    fn set_sub_speed(&self, _value: f64) {}

    fn set_sub_delay(&self, _value: f64) {}

    fn set_sub_bold(&self, _value: bool) {}

    fn set_sub_italic(&self, _value: bool) {}

    fn set_sub_font(&self, _value: &str) {}

    fn set_sub_color(&self, _value: &str) {}

    fn set_sub_border_color(&self, _value: &str) {}

    fn set_sub_back_color(&self, _value: &str) {}

    fn set_sub_border_style(&self, _value: &str) {}

    fn set_sub_border_size(&self, _value: f64) {}

    fn set_sub_shadow_offset(&self, _value: f64) {}

    fn set_audio_delay(&self, _value: f64) {}

    fn set_audio_channels(&self, _value: &str) {}

    fn set_audio_pan(&self, _value: &str) {}

    fn clear_audio_pan(&self) {}

    fn set_scale(&self, _value: &str) {}

    fn set_deband(&self, _value: bool) {}

    fn set_deband_iterations(&self, _value: i64) {}

    fn set_deband_threshold(&self, _value: i64) {}

    fn set_deband_range(&self, _value: i64) {}

    fn set_deband_grain(&self, _value: i64) {}

    fn set_deinterlace(&self, _value: bool) {}

    fn set_hwdec(&self, _value: &str) {}

    fn set_panscan(&self, _value: f64) {}

    fn set_stretch_image_subs_to_screen(&self, _value: bool) {}

    fn set_demuxer_max_bytes(&self, _value: &str) {}

    fn set_cache_secs(&self, _value: f64) {}
}

impl std::fmt::Display for dyn VideoBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VideoBackend")
    }
}
