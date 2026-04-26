use std::{cell::RefCell, sync::OnceLock};

use gstreamer as gst;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::contexted::ContextedGstPlayer;
use crate::{
    BoxedFuture,
    video::backend::{TrackKind, TrackSelection, VideoBackend},
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum GstVideoError {
    #[error("failed to initialize gstreamer")]
    InitFailed,
    #[error("failed to create gtk4paintablesink")]
    MissingPaintableSink,
}

mod imp {
    use super::*;

    pub struct GstVideo {
        pub picture: gtk::Picture,
        pub sink: gst::Element,
        pub player: ContextedGstPlayer,
        pub paintable_notify: RefCell<Option<glib::SignalHandlerId>>,
    }

    impl Default for GstVideo {
        fn default() -> Self {
            gst::init().expect("failed to initialize GStreamer");

            let sink = gst::ElementFactory::make("gtk4paintablesink")
                .build()
                .expect("failed to create gtk4paintablesink");

            let picture = gtk::Picture::new();
            picture.set_hexpand(true);
            picture.set_vexpand(true);
            picture.set_can_shrink(true);
            picture.set_content_fit(gtk::ContentFit::Contain);

            if let Some(paintable) = sink.property::<Option<gdk::Paintable>>("paintable") {
                picture.set_paintable(Some(&paintable));
            }

            let paintable_notify = sink.connect_notify_local(
                Some("paintable"),
                glib::clone!(
                    #[weak]
                    picture,
                    move |obj, _| {
                        let paintable = obj.property::<Option<gdk::Paintable>>("paintable");
                        picture.set_paintable(paintable.as_ref());
                    }
                ),
            );

            let player = ContextedGstPlayer::with_video_sink(&sink);

            Self {
                picture,
                sink,
                player,
                paintable_notify: RefCell::new(Some(paintable_notify)),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GstVideo {
        const NAME: &'static str = "MutsumiGstVideo";
        type Type = super::GstVideo;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for GstVideo {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.set_orientation(gtk::Orientation::Vertical);
            obj.set_spacing(0);
            obj.set_hexpand(true);
            obj.set_vexpand(true);

            obj.append(&self.picture);
        }

        fn dispose(&self) {
            if let Some(handler_id) = self.paintable_notify.borrow_mut().take() {
                self.sink.disconnect(handler_id);
            }

            self.player.shutdown();

            if self.picture.parent().is_some() {
                self.picture.unparent();
            }
        }
    }

    impl WidgetImpl for GstVideo {}
    impl BoxImpl for GstVideo {}
}

glib::wrapper! {
    pub struct GstVideo(ObjectSubclass<imp::GstVideo>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl GstVideo {
    pub fn new() -> Result<Self, GstVideoError> {
        gst::init().map_err(|_| GstVideoError::InitFailed)?;

        let _ = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .map_err(|_| GstVideoError::MissingPaintableSink)?;

        Ok(glib::Object::builder().build())
    }

    pub fn widget(&self) -> &gtk::Box {
        self.upcast_ref()
    }

    pub fn picture(&self) -> &gtk::Picture {
        &self.imp().picture
    }

    pub fn sink(&self) -> &gst::Element {
        &self.imp().sink
    }

    pub fn player(&self) -> &ContextedGstPlayer {
        &self.imp().player
    }

    pub fn play(&self, url: &str, percentage: f64) {
        self.player().play(url, percentage);
    }

    pub fn load_video(&self, url: &str) {
        self.player().load_video(url);
    }

    pub fn pause(&self, pause: bool) {
        self.player().pause(pause);
    }

    pub fn command_pause(&self) {
        self.player().command_pause();
    }

    pub fn stop(&self) {
        self.player().stop();
    }

    pub fn shutdown(&self) {
        self.player().shutdown();
    }

    pub fn add_sub(&self, url: &str) {
        self.player().add_sub(url);
    }

    pub fn seek_forward(&self, value: i64) {
        self.player().seek_forward(value);
    }

    pub fn seek_backward(&self, value: i64) {
        self.player().seek_backward(value);
    }

    pub fn set_position(&self, value: f64) {
        self.player().set_position(value);
    }

    pub fn set_percent_position(&self, value: f64) {
        self.player().set_percent_position(value);
    }

    pub fn set_start(&self, percentage: f64) {
        self.player().set_start(percentage);
    }

    pub async fn position(&self) -> f64 {
        self.player().position().await
    }

    pub async fn duration(&self) -> f64 {
        self.player().duration().await
    }

    pub async fn paused(&self) -> bool {
        self.player().paused().await
    }

    pub fn set_volume(&self, value: i64) {
        self.player().set_volume(value);
    }

    pub fn volume_scroll(&self, value: i64) {
        self.player().volume_scroll(value);
    }

    pub fn set_speed(&self, value: f64) {
        self.player().set_speed(value);
    }

    pub fn set_aid(&self, value: TrackSelection) {
        self.player().set_aid(value);
    }

    pub fn set_sid(&self, value: TrackSelection) {
        self.player().set_sid(value);
    }

    pub fn set_keep_aspect_ratio(&self, keep: bool) {
        self.picture().set_content_fit(if keep {
            gtk::ContentFit::Contain
        } else {
            gtk::ContentFit::Fill
        });
    }

    pub fn bind_paintable_now(&self) {
        let paintable = self.sink().property::<Option<gdk::Paintable>>("paintable");
        self.picture().set_paintable(paintable.as_ref());
    }

    pub fn connect_realize_bind_paintable(&self) {
        self.connect_realize(glib::clone!(
            #[weak(rename_to = sink)]
            self.imp().sink,
            #[weak(rename_to = picture)]
            self.imp().picture,
            move |_| {
                let paintable = sink.property::<Option<gdk::Paintable>>("paintable");
                picture.set_paintable(paintable.as_ref());
            }
        ));
    }
}

impl Default for GstVideo {
    fn default() -> Self {
        Self::new().expect("failed to initialize GstVideo")
    }
}

impl VideoBackend for GstVideo {
    fn name() -> &'static str {
        "GStreamer"
    }

    fn play(&self, url: &str, percentage: f64) {
        GstVideo::play(self, url, percentage);
    }

    fn shutdown(&self) {
        GstVideo::shutdown(self);
    }

    fn stop(&self) {
        GstVideo::stop(self);
    }

    fn load_video(&self, url: &str) {
        GstVideo::load_video(self, url);
    }

    fn add_sub(&self, url: &str) {
        GstVideo::add_sub(self, url);
    }

    fn pause(&self, pause: bool) {
        GstVideo::pause(self, pause);
    }

    fn command_pause(&self) {
        GstVideo::command_pause(self);
    }

    fn set_position(&self, value: f64) {
        GstVideo::set_position(self, value);
    }

    fn set_percent_position(&self, value: f64) {
        GstVideo::set_percent_position(self, value);
    }

    fn set_start(&self, percentage: f64) {
        GstVideo::set_start(self, percentage);
    }

    fn set_volume(&self, value: i64) {
        GstVideo::set_volume(self, value);
    }

    fn volume_scroll(&self, value: i64) {
        GstVideo::volume_scroll(self, value);
    }

    fn set_speed(&self, value: f64) {
        GstVideo::set_speed(self, value);
    }

    fn seek_forward(&self, value: i64) {
        GstVideo::seek_forward(self, value);
    }

    fn seek_backward(&self, value: i64) {
        GstVideo::seek_backward(self, value);
    }

    fn position(&self) -> BoxedFuture<'_, f64> {
        Box::pin(async { GstVideo::position(self).await })
    }

    fn duration(&self) -> BoxedFuture<'_, f64> {
        Box::pin(async { GstVideo::duration(self).await })
    }

    fn paused(&self) -> BoxedFuture<'_, bool> {
        Box::pin(async { GstVideo::paused(self).await })
    }

    fn set_aid(&self, value: TrackSelection) {
        GstVideo::set_aid(self, value);
    }

    fn set_sid(&self, value: TrackSelection) {
        GstVideo::set_sid(self, value);
    }

    fn set_keep_aspect_ratio(&self, keep: bool) {
        GstVideo::set_keep_aspect_ratio(self, keep);
    }

    fn set_slang(&self, _value: String) {
        // Gstreamer does not support this
    }

    fn get_track_id(&self, _kind: TrackKind) -> BoxedFuture<'_, i64> {
        // Gstreamer does not support this
        Box::pin(async { -1 })
    }

    fn display_stats_toggle(&self) {
        // Gstreamer does not support this
    }
}
