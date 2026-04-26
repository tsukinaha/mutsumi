use std::{cell::Cell, rc::Rc};

use gst::prelude::*;
use gstreamer as gst;
use gstreamer::bus::BusWatchGuard;
use gtk::{gio, prelude::FileExt};
use tracing::info;

use crate::TrackSelection;

use super::{GST_EVENT_CHANNEL, ListenEvent};

struct Inner {
    playbin: gst::Element,
    _video_sink: gst::Element,
    _bus_watch: BusWatchGuard,
    paused: Cell<bool>,
    volume_percent: Cell<i64>,
    speed: Cell<f64>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

#[derive(Clone)]
pub struct ContextedGstPlayer {
    inner: Rc<Inner>,
}

impl Default for ContextedGstPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextedGstPlayer {
    pub fn new() -> Self {
        gst::init().expect("failed to initialize gstreamer");

        let video_sink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .expect("failed to create gtk4paintablesink");

        Self::with_video_sink(&video_sink)
    }

    pub fn with_video_sink(video_sink: &gst::Element) -> Self {
        gst::init().expect("failed to initialize gstreamer");

        let playbin = gst::ElementFactory::make("playbin")
            .build()
            .expect("failed to create playbin");

        playbin.set_property("video-sink", video_sink);

        let bus_watch = install_bus_watch(&playbin);

        Self {
            inner: Rc::new(Inner {
                playbin,
                _video_sink: video_sink.clone(),
                _bus_watch: bus_watch,
                paused: Cell::new(false),
                volume_percent: Cell::new(100),
                speed: Cell::new(1.0),
            }),
        }
    }

    pub fn shutdown(&self) {
        let _ = self.inner.playbin.set_state(gst::State::Null);
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Shutdown);
    }

    pub fn stop(&self) {
        let _ = self.inner.playbin.set_state(gst::State::Ready);
        self.inner.paused.set(true);
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Pause(true));
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::TimePos(0.0));
    }

    pub fn load_video(&self, url: &str) {
        info!("Now Playing: {}", url);
        let _ = self.inner.playbin.set_state(gst::State::Ready);
        self.inner
            .playbin
            .set_property("suburi", Option::<String>::None);
        self.inner.playbin.set_property("uri", normalize_uri(url));
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::StartFile);
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::TimePos(0.0));
    }

    pub fn play(&self, url: &str, percentage: f64) {
        self.load_video(url);
        self.set_start(percentage);
        self.pause(false);
    }

    pub fn add_sub(&self, url: &str) {
        self.inner
            .playbin
            .set_property("suburi", normalize_uri(url));
        let _ = GST_EVENT_CHANNEL
            .tx
            .send(ListenEvent::SubtitleAdded(url.to_owned()));
    }

    pub fn pause(&self, pause: bool) {
        self.inner.paused.set(pause);
        let state = if pause {
            gst::State::Paused
        } else {
            gst::State::Playing
        };
        let _ = self.inner.playbin.set_state(state);
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Pause(pause));
    }

    pub fn command_pause(&self) {
        self.pause(!self.inner.paused.get());
    }

    pub fn set_position(&self, value: f64) {
        seek_to_seconds(&self.inner.playbin, value.max(0.0));
    }

    pub fn set_percent_position(&self, value: f64) {
        let percent = value.clamp(0.0, 100.0);
        if let Some(duration) = query_duration_seconds(&self.inner.playbin) {
            seek_to_seconds(&self.inner.playbin, duration * percent / 100.0);
        }
    }

    pub fn set_start(&self, percentage: f64) {
        self.set_percent_position(percentage);
    }

    pub async fn position(&self) -> f64 {
        query_position_seconds(&self.inner.playbin).unwrap_or(0.0)
    }

    pub async fn duration(&self) -> f64 {
        query_duration_seconds(&self.inner.playbin).unwrap_or(0.0)
    }

    pub async fn paused(&self) -> bool {
        let paused = self.inner.playbin.current_state() == gst::State::Paused;
        self.inner.paused.set(paused);
        paused
    }

    pub fn set_volume(&self, volume: i64) {
        let volume = volume.clamp(0, 200);
        self.inner.volume_percent.set(volume);
        let linear = volume as f64 / 100.0;
        self.inner.playbin.set_property("volume", linear);
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Volume(linear));
    }

    pub fn volume_scroll(&self, value: i64) {
        let next = self.inner.volume_percent.get().saturating_add(value);
        self.set_volume(next);
    }

    pub fn set_speed(&self, speed: f64) {
        let speed = if speed <= 0.0 { 1.0 } else { speed };
        self.inner.speed.set(speed);

        let position = self
            .inner
            .playbin
            .query_position::<gst::ClockTime>()
            .unwrap_or(gst::ClockTime::ZERO);

        let _ = self.inner.playbin.seek(
            speed,
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::SeekType::Set,
            position,
            gst::SeekType::None,
            gst::ClockTime::NONE,
        );

        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Speed(speed));
    }

    pub fn speed(&self) -> f64 {
        self.inner.speed.get()
    }

    pub fn set_aid(&self, aid: TrackSelection) {
        let value = match aid {
            TrackSelection::Track(index) => index,
            TrackSelection::None => -1,
        };
        self.inner.playbin.set_property("current-audio", value);
    }

    pub fn set_sid(&self, sid: TrackSelection) {
        let value = match sid {
            TrackSelection::Track(index) => index,
            TrackSelection::None => -1,
        };
        self.inner.playbin.set_property("current-text", value);
    }

    pub fn seek_forward(&self, value: i64) {
        self.seek_relative_seconds(value as f64);
    }

    pub fn seek_backward(&self, value: i64) {
        self.seek_relative_seconds(-(value as f64));
    }

    pub fn seek_relative_seconds(&self, delta: f64) {
        let current = query_position_seconds(&self.inner.playbin).unwrap_or(0.0);
        seek_to_seconds(&self.inner.playbin, (current + delta).max(0.0));
    }
}

fn install_bus_watch(playbin: &gst::Element) -> BusWatchGuard {
    let bus = playbin.bus().expect("playbin should always have a bus");
    let watched = playbin.clone();

    bus.add_watch_local(move |_bus, msg| {
        use glib::ControlFlow;
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => {
                let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Eof);
            }
            MessageView::Error(err) => {
                let src = err
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let debug = err.debug().unwrap_or_default();
                let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Error(format!(
                    "{src}: {} ({debug})",
                    err.error()
                )));
            }
            MessageView::Warning(warn) => {
                let src = warn
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let debug = warn.debug().unwrap_or_default();
                let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Warning(format!(
                    "{src}: {} ({debug})",
                    warn.error()
                )));
            }
            MessageView::Buffering(buffering) => {
                let percent = buffering.percent();
                let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Buffering(percent));
                let _ = GST_EVENT_CHANNEL
                    .tx
                    .send(ListenEvent::PausedForCache(percent < 100));
            }
            MessageView::DurationChanged(..) => {
                let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Duration(
                    query_duration_seconds(&watched).unwrap_or(0.0),
                ));
            }
            MessageView::AsyncDone(..) => {
                let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::SeekDone);
                if let Some(pos) = query_position_seconds(&watched) {
                    let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::TimePos(pos));
                }
            }
            MessageView::StateChanged(state) => {
                let watched_path = watched.path_string();
                let is_ours = msg
                    .src()
                    .map(|src| src.path_string() == watched_path)
                    .unwrap_or(false);

                if is_ours {
                    let current = state.current();

                    let _ = GST_EVENT_CHANNEL
                        .tx
                        .send(ListenEvent::StateChanged(current));
                    let _ = GST_EVENT_CHANNEL
                        .tx
                        .send(ListenEvent::Pause(current == gst::State::Paused));

                    if current == gst::State::Playing {
                        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::PlaybackRestart);
                    }

                    if let Some(duration) = query_duration_seconds(&watched) {
                        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Duration(duration));
                    }

                    if let Some(position) = query_position_seconds(&watched) {
                        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::TimePos(position));
                    }
                }
            }
            _ => {}
        }

        ControlFlow::Continue
    })
    .expect("failed to install GstBus watch")
}

fn seek_to_seconds(playbin: &gst::Element, seconds: f64) {
    let target = seconds.max(0.0);
    let result = playbin.seek_simple(
        gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::ACCURATE,
        seconds_to_clock_time(target),
    );

    if result.is_ok() {
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::Seek);
        let _ = GST_EVENT_CHANNEL.tx.send(ListenEvent::TimePos(target));
    }
}

fn query_position_seconds(playbin: &gst::Element) -> Option<f64> {
    playbin
        .query_position::<gst::ClockTime>()
        .map(clock_time_to_seconds)
}

fn query_duration_seconds(playbin: &gst::Element) -> Option<f64> {
    playbin
        .query_duration::<gst::ClockTime>()
        .map(clock_time_to_seconds)
}

fn clock_time_to_seconds(value: gst::ClockTime) -> f64 {
    value.nseconds() as f64 / 1_000_000_000.0
}

fn seconds_to_clock_time(value: f64) -> gst::ClockTime {
    if !value.is_finite() || value <= 0.0 {
        return gst::ClockTime::ZERO;
    }

    gst::ClockTime::from_nseconds((value * 1_000_000_000.0) as u64)
}

fn normalize_uri(value: &str) -> String {
    if value.contains("://") {
        value.to_string()
    } else {
        gio::File::for_path(value).uri().to_string()
    }
}
