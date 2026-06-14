use adw::{prelude::*, subclass::prelude::*};
use glib::spawn_future_local;
use gtk::{Builder, CompositeTemplate, PopoverMenu, gdk::Rectangle, gio, glib};

use crate::{
    ChapterList, DanmakuTrack, ListenEvent, MPV_EVENT_CHANNEL, MpvActor, MpvTrack, MpvTracks,
    MutsumiVideoPlayer, PlayParams, TrackKind, TrackSelection,
    control::{ControlSidebar, GlobalToast, MenuActions, VideoScale, VolumeBar, format_duration},
};

/// Minimum interval between two pointer motion events that reveal the
/// overlay, in microseconds.
const MIN_MOTION_TIME: i64 = 100000;
const NEXT_CHAPTER_KEYVAL: u32 = 65365; // Page_Up
const PREV_CHAPTER_KEYVAL: u32 = 65366; // Page_Down

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Default, CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/mutsumi/ui/player.ui")]
    #[properties(wrapper_type = super::MutsumiPlayer)]
    pub struct MutsumiPlayer {
        #[property(get, set)]
        pub video_title: RefCell<String>,
        #[property(get, set)]
        pub video_subtitle: RefCell<String>,
        #[property(get, set = Self::set_fullscreened, explicit_notify)]
        pub fullscreened: Cell<bool>,
        #[property(get, set = Self::set_paused)]
        pub paused: Cell<bool>,

        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub control_sidebar: TemplateChild<ControlSidebar>,
        #[template_child]
        pub video: TemplateChild<MutsumiVideoPlayer>,
        #[template_child]
        pub volume_bar: TemplateChild<VolumeBar>,
        #[template_child]
        pub loading_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub network_speed_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub top_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub bottom_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub play_pause_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub video_scale: TemplateChild<VideoScale>,
        #[template_child]
        pub progress_time_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub duration_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub playback_speed_indicator: TemplateChild<gtk::Button>,
        #[template_child]
        pub playback_speed_button_content: TemplateChild<adw::ButtonContent>,
        #[template_child]
        pub audio_tracks_menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub subtitle_tracks_menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub volume_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub volume_adj: TemplateChild<gtk::Adjustment>,
        #[template_child]
        pub sub_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub audio_listbox: TemplateChild<gtk::ListBox>,

        #[template_child]
        pub danmakw: TemplateChild<crate::Danmakw>,

        pub menu_actions: MenuActions,
        pub context_popover: RefCell<Option<PopoverMenu>>,

        pub fade_timeout: RefCell<Option<glib::source::SourceId>>,
        pub x: Cell<f64>,
        pub y: Cell<f64>,
        pub last_motion_time: Cell<i64>,

        pub shortcuts_dialog: OnceCell<adw::ShortcutsDialog>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MutsumiPlayer {
        const NAME: &'static str = "MutsumiPlayer";
        type Type = super::MutsumiPlayer;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            MutsumiVideoPlayer::ensure_type();
            ControlSidebar::ensure_type();
            VideoScale::ensure_type();
            VolumeBar::ensure_type();

            klass.bind_template();
            klass.bind_template_instance_callbacks();

            klass.install_action("player.play-pause", None, |obj, _, _| {
                obj.on_play_pause();
            });
            klass.install_action("player.backward", None, |obj, _, _| {
                obj.on_backward();
            });
            klass.install_action("player.forward", None, |obj, _, _| {
                obj.on_forward();
            });
            klass.install_action("player.chapter-prev", None, |obj, _, _| {
                obj.chapter_prev();
            });
            klass.install_action("player.chapter-next", None, |obj, _, _| {
                obj.chapter_next();
            });
            klass.install_action("player.show-info", None, |obj, _, _| {
                obj.imp().video.display_stats_toggle();
            });
            klass.install_action("player.show-settings", None, |obj, _, _| {
                let split_view = &obj.imp().split_view;
                split_view.set_show_sidebar(!split_view.shows_sidebar());
            });
            klass.install_action("player.show-shortcuts", None, |obj, _, _| {
                obj.show_shortcuts_dialog();
            });
            klass.install_action("player.toggle-fullscreen", None, |obj, _, _| {
                if let Some(window) = obj.root().and_downcast::<gtk::Window>() {
                    window.set_fullscreened(!window.is_fullscreen());
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MutsumiPlayer {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();

            self.control_sidebar.set_player(Some(&self.video.get()));
            self.video_scale.set_player(Some(&self.video.get()));
            self.video_scale.set_danmakw(Some(&self.danmakw.get()));

            obj.setup_context_menu();

            obj.connect_root_notify(|obj| {
                if let Some(window) = obj.root().and_downcast::<gtk::Window>() {
                    window
                        .bind_property("fullscreened", obj, "fullscreened")
                        .sync_create()
                        .build();
                }
            });

            obj.listen_events();
        }

        fn dispose(&self) {
            if let Some(popover) = self.context_popover.take() {
                popover.unparent();
            }
        }
    }

    impl WidgetImpl for MutsumiPlayer {}
    impl BinImpl for MutsumiPlayer {}

    impl MutsumiPlayer {
        fn set_fullscreened(&self, fullscreened: bool) {
            if fullscreened == self.fullscreened.get() {
                return;
            }

            self.fullscreened.set(fullscreened);
            self.obj().notify_fullscreened();
        }

        fn set_paused(&self, paused: bool) {
            let play_pause_image = self.play_pause_image.get();
            if paused {
                play_pause_image.set_icon_name(Some("media-playback-start-symbolic"));
                play_pause_image.set_tooltip_text(Some("Play"));
            } else {
                play_pause_image.set_icon_name(Some("media-playback-pause-symbolic"));
                play_pause_image.set_tooltip_text(Some("Pause"));
            }
            self.menu_actions.set_paused(paused);
            self.paused.set(paused);

            // seeking
            if !self.loading_box.is_visible() {
                self.danmakw.set_paused(paused);
            }
        }
    }
}

glib::wrapper! {
    /// A self-contained video player widget: video output plus on-screen
    /// controls, context menu and an advanced settings sidebar.
    pub struct MutsumiPlayer(ObjectSubclass<imp::MutsumiPlayer>)
        @extends gtk::Widget, adw::Bin,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for MutsumiPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[gtk::template_callbacks]
impl MutsumiPlayer {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn player(&self) -> MutsumiVideoPlayer {
        self.imp().video.get()
    }

    pub fn mpv(&self) -> MpvActor {
        self.imp().video.get().backend_ref().mpv().mpv
    }

    pub fn play(&self, params: &PlayParams) {
        if let Some(ref title) = params.title {
            self.set_video_title(title.to_owned());
        }

        if let Some(ref subtitle) = params.subtitle {
            self.set_video_subtitle(subtitle.to_owned());
        }

        self.imp().video.play(params);
    }

    pub fn stop(&self) {
        self.imp().video.stop();
    }

    pub fn reveal_controls(&self, reveal: bool) {
        self.set_reveal_overlay(reveal);
        if reveal {
            self.reset_fade_timeout();
        }
    }

    fn setup_context_menu(&self) {
        let imp = self.imp();
        let builder = Builder::from_resource("/io/github/mutsumi/ui/menu.ui");
        let Some(menu) = builder.object::<gio::MenuModel>("player-menu") else {
            tracing::error!("Failed to load player context menu model");
            return;
        };

        let popover = PopoverMenu::builder()
            .menu_model(&menu)
            .halign(gtk::Align::Start)
            .has_arrow(false)
            .build();
        popover.set_parent(self);
        popover.add_child(&imp.menu_actions, "menu-actions");
        imp.context_popover.replace(Some(popover));
    }

    fn listen_events(&self) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                while let Ok(value) = MPV_EVENT_CHANNEL.rx.recv_async().await {
                    match value {
                        ListenEvent::Duration(value) => {
                            obj.update_duration(value);
                        }
                        ListenEvent::PausedForCache(true) | ListenEvent::Seek => {
                            obj.update_seeking(true);
                        }
                        ListenEvent::PausedForCache(false) | ListenEvent::PlaybackRestart => {
                            obj.update_seeking(false);
                        }
                        ListenEvent::StartFile => {
                            obj.on_start_file();
                        }
                        ListenEvent::Eof(_) => {
                            obj.update_seeking(false);
                        }
                        ListenEvent::Error(value) => {
                            obj.toast(value);
                        }
                        ListenEvent::Pause(value) => {
                            obj.set_paused(value);
                        }
                        ListenEvent::CacheSpeed(value) => {
                            obj.on_cache_speed_update(value);
                        }
                        ListenEvent::TrackList(value) => {
                            obj.on_track_list(value);
                        }
                        ListenEvent::Volume(value) => {
                            obj.on_volume_update(value);
                        }
                        ListenEvent::Speed(value) => {
                            obj.on_speed_update(value);
                        }
                        ListenEvent::Shutdown => {
                            tracing::warn!("MPV has been shut down");
                        }
                        ListenEvent::DemuxerCacheTime(value) => {
                            obj.imp().video_scale.set_cache_end_time(value);
                        }
                        ListenEvent::TimePos(value) => {
                            obj.on_time_pos(value);
                        }
                        ListenEvent::ChapterList(value) => {
                            obj.on_chapter_list(value);
                        }
                    }
                }
            }
        ));
    }

    fn on_start_file(&self) {
        self.imp().video_scale.reset_scale();
        self.update_seeking(true);
    }

    fn update_duration(&self, value: f64) {
        let imp = self.imp();
        let duration = format_duration(value as i64);
        let width_chars = duration.chars().count() as i32;
        imp.video_scale.set_range(0.0, value);
        imp.progress_time_label.set_width_chars(width_chars);
        imp.duration_label.set_width_chars(width_chars);
        imp.duration_label.set_text(&duration);
    }

    fn update_seeking(&self, seeking: bool) {
        self.imp().loading_box.set_visible(seeking);

        if self.paused() {
            self.queue_draw();
            return;
        }

        self.imp().danmakw.set_paused(seeking);
    }

    fn on_cache_speed_update(&self, value: i64) {
        let label = &self.imp().network_speed_label;
        if value >= 2 * 1024 * 1024 {
            label.set_text(&format!("{:.2} MiB/s", value as f64 / (1024.0 * 1024.0)));
        } else {
            label.set_text(&format!("{} KiB/s", value / 1024));
        }
    }

    fn on_volume_update(&self, value: i64) {
        let imp = self.imp();
        imp.volume_adj.set_value(value as f64);
        imp.volume_bar.set_level(value as f64 / 100.0);

        let icon_name = match value {
            0 => "audio-volume-muted-symbolic",
            value if value < 33 => "audio-volume-low-symbolic",
            value if value < 66 => "audio-volume-medium-symbolic",
            _ => "audio-volume-high-symbolic",
        };
        imp.volume_button.set_icon_name(icon_name);
    }

    fn on_speed_update(&self, value: f64) {
        let imp = self.imp();
        imp.playback_speed_button_content
            .set_label(&format!("{value:.2}x"));
        imp.playback_speed_indicator
            .set_visible((value * 100.0).round() as i64 != 100);
        imp.control_sidebar.set_playback_speed(value);
    }

    fn shortcuts_dialog_is_presented(&self) -> bool {
        let Some(dialog) = self.imp().shortcuts_dialog.get() else {
            return false;
        };

        dialog.parent().is_some()
    }

    fn show_shortcuts_dialog(&self) {
        let dialog = self
            .imp()
            .shortcuts_dialog
            .get_or_init(|| self.create_shortcuts_dialog());

        dialog.present(Some(self));
    }

    fn create_shortcuts_dialog(&self) -> adw::ShortcutsDialog {
        let builder = Builder::from_resource("/io/github/mutsumi/ui/shortcuts.ui");
        builder
            .object::<adw::ShortcutsDialog>("shortcuts_dialog")
            .expect("Failed to load shortcuts dialog")
    }

    fn on_time_pos(&self, value: i64) {
        let imp = self.imp();
        if !imp.video_scale.is_dragging() {
            imp.video_scale.set_value(value as f64);
        }
    }

    fn on_chapter_list(&self, value: ChapterList) {
        self.imp().video_scale.set_chapter_list(value);
    }

    fn on_track_list(&self, value: MpvTracks) {
        let imp = self.imp();
        self.bind_tracks(
            value.audio_tracks,
            &imp.audio_listbox.get(),
            TrackKind::Audio,
        );

        self.bind_tracks(
            value.sub_tracks,
            &imp.sub_listbox.get(),
            TrackKind::Subtitle,
        );

        if let Some(danmaku_track) = value.danmaku_track {
            self.bind_danmaku(danmaku_track);
        }
    }

    fn bind_tracks(&self, tracks: Vec<MpvTrack>, listbox: &gtk::ListBox, kind: TrackKind) {
        let obj = self.clone();
        let listbox = listbox.clone();

        glib::spawn_future_local(async move {
            let current_id = obj.imp().video.get_track_id(kind).await;

            while let Some(row) = listbox.first_child() {
                listbox.remove(&row);
            }

            let group_check = obj.append_track_row(&listbox, "None", "", 0 == current_id, None, {
                let obj = obj.clone();
                move || obj.set_track(kind, 0)
            });

            for track in tracks {
                let title = if track.title == "unknown" {
                    format!("Track {}", track.id)
                } else {
                    track.title.clone()
                };
                let id = track.id;
                obj.append_track_row(
                    &listbox,
                    &title,
                    &track.lang,
                    id == current_id,
                    Some(&group_check),
                    {
                        let obj = obj.clone();
                        move || obj.set_track(kind, id)
                    },
                );
            }
        });
    }

    fn append_track_row(
        &self,
        listbox: &gtk::ListBox,
        title: &str,
        subtitle: &str,
        active: bool,
        group: Option<&gtk::CheckButton>,
        on_activated: impl Fn() + 'static,
    ) -> gtk::CheckButton {
        let check = gtk::CheckButton::builder()
            .valign(gtk::Align::Center)
            .can_focus(false)
            .build();
        check.set_group(group);
        check.set_active(active);

        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(title))
            .subtitle(glib::markup_escape_text(subtitle))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);

        check.connect_toggled(move |check| {
            if check.is_active() {
                on_activated();
            }
        });

        listbox.append(&row);
        check
    }

    fn set_track(&self, kind: TrackKind, track_id: i64) {
        let selection = if track_id == 0 {
            TrackSelection::None
        } else {
            TrackSelection::Track(track_id)
        };

        let video = &self.imp().video;
        match kind {
            TrackKind::Audio => video.set_aid(selection),
            TrackKind::Subtitle => video.set_sid(selection),
            TrackKind::Video => {}
        }
    }

    #[template_callback]
    fn on_progress_value_changed(&self, progress_scale: &VideoScale) {
        let label = &self.imp().progress_time_label;
        label.set_text(&format_duration(progress_scale.value() as i64));
    }

    #[template_callback]
    fn on_volume_scale_value_changed(&self, scale: &gtk::Scale) {
        self.imp().video.set_volume(scale.value() as i64);
    }

    #[template_callback]
    fn playback_speed_indicator_cb(&self, _btn: &gtk::Button) {
        self.imp().video.set_speed(1.0);
    }

    #[template_callback]
    fn video_scroll_cb(&self, _dx: f64, dy: f64) -> bool {
        self.imp().video.volume_scroll(-dy as i64 * 5);
        true
    }

    #[template_callback]
    fn left_click_cb(&self, _n: i32, _x: f64, _y: f64) {
        self.grab_focus();
        self.imp().video.command_pause();
    }

    #[template_callback]
    fn right_click_cb(&self, _n: i32, x: f64, y: f64) {
        if let Some(popover) = self.imp().context_popover.borrow().as_ref() {
            popover.set_pointing_to(Some(&Rectangle::new(x as i32, y as i32, 0, 0)));
            popover.popup();
        };
    }

    #[template_callback]
    fn on_key_pressed(&self, keyval: u32, _keycode: u32, state: gtk::gdk::ModifierType) -> bool {
        if self.imp().split_view.shows_sidebar() {
            return false;
        }

        self.imp().video.press_key(keyval, state);
        true
    }

    #[template_callback]
    fn on_key_released(&self, keyval: u32, _keycode: u32, state: gtk::gdk::ModifierType) {
        if self.imp().split_view.shows_sidebar() {
            return;
        }

        self.imp().video.release_key(keyval, state);
    }

    #[template_callback]
    fn on_motion(&self, x: f64, y: f64) {
        let imp = self.imp();

        let old_x = imp.x.get();
        let old_y = imp.y.get();

        if old_x == x && old_y == y {
            return;
        }

        imp.x.set(x);
        imp.y.set(y);

        let now = glib::monotonic_time();

        if now - imp.last_motion_time.get() < MIN_MOTION_TIME {
            return;
        }

        let is_threshold = (old_x - x).abs() > 3.0 || (old_y - y).abs() > 3.0;

        if is_threshold {
            if !self.toolbar_revealed() {
                self.set_reveal_overlay(true);
            }

            self.reset_fade_timeout();

            imp.last_motion_time.set(now);
        }
    }

    #[template_callback]
    fn on_leave(&self) {
        let imp = self.imp();
        imp.x.set(-1.0);
        imp.y.set(-1.0);

        if self.toolbar_revealed() && imp.fade_timeout.borrow().is_none() {
            self.reset_fade_timeout();
        }
    }

    #[template_callback]
    fn on_enter(&self, _x: f64, _y: f64) {
        if self.toolbar_revealed() {
            self.reset_fade_timeout();
        } else {
            self.set_reveal_overlay(true);
        }
    }

    fn on_play_pause(&self) {
        self.imp().video.command_pause();
    }

    fn on_backward(&self) {
        let imp = self.imp();
        imp.video
            .seek_backward(imp.control_sidebar.seek_backward_step());
    }

    fn on_forward(&self) {
        let imp = self.imp();
        imp.video
            .seek_forward(imp.control_sidebar.seek_forward_step());
    }

    fn chapter_prev(&self) {
        self.imp()
            .video
            .press_key(PREV_CHAPTER_KEYVAL, gtk::gdk::ModifierType::empty());
    }

    fn chapter_next(&self) {
        self.imp()
            .video
            .press_key(NEXT_CHAPTER_KEYVAL, gtk::gdk::ModifierType::empty());
    }

    fn toolbar_revealed(&self) -> bool {
        self.imp().bottom_revealer.is_child_revealed()
    }

    fn reset_fade_timeout(&self) {
        let imp = self.imp();
        if let Some(timeout) = imp.fade_timeout.take() {
            timeout.remove();
        }
        let timeout = glib::timeout_add_seconds_local_once(
            3,
            glib::clone!(
                #[weak(rename_to = obj)]
                self,
                move || {
                    obj.fade_overlay_delay_cb();
                }
            ),
        );
        imp.fade_timeout.replace(Some(timeout));
    }

    fn fade_overlay_delay_cb(&self) {
        self.imp().fade_timeout.replace(None);

        if self.toolbar_revealed() && self.can_fade_overlay() {
            self.set_reveal_overlay(false);
        }
    }

    fn can_fade_overlay(&self) -> bool {
        let imp = self.imp();

        let x = imp.x.get();
        let y = imp.y.get();

        if let Some(widget) = self.pick(x, y, gtk::PickFlags::DEFAULT) {
            if widget.downcast_ref::<crate::Danmakw>().is_none() {
                return false;
            }
        }

        if self.shortcuts_dialog_is_presented() {
            return false;
        }

        true
    }

    fn set_reveal_overlay(&self, reveal: bool) {
        let imp = self.imp();
        imp.bottom_revealer.set_reveal_child(reveal);
        imp.top_revealer.set_reveal_child(reveal);

        let Some(surface) = self.native().and_then(|f| f.surface()) else {
            return;
        };
        let cursor = if reveal {
            gtk::gdk::Cursor::from_name("default", None)
        } else {
            gtk::gdk::Cursor::from_name("none", None)
        };

        surface.set_cursor(cursor.as_ref());
    }

    pub fn bind_danmaku(&self, track: DanmakuTrack) {
        let external_url = track.external_url.clone();

        spawn_future_local(glib::clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                match fetch_url_content(&external_url).await {
                    Ok(content) => {
                        let Ok(danmaku) = crate::parse_bilibili_xml(&content) else {
                            return;
                        };
                        obj.imp().danmakw.load_danmaku(danmaku);
                    }
                    Err(e) => {
                        tracing::error!("Failed to load danmaku from {}: {}", external_url, e);
                        obj.toast(format!("Failed to load danmaku: {}", e));
                    }
                }
            }
        ));
    }
}

async fn fetch_url_content(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let file = gio::File::for_uri(url);

    let (content_bytes, _etag) = file.load_contents_future().await?;

    let content = String::from_utf8(content_bytes.to_vec())?;

    Ok(content)
}
