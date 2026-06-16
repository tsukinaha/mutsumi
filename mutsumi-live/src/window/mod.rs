use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};
use mutsumi::{Color, DanmakuMode, MutsumiPlayer, PlayParams, PlaySource};

use crate::PlayList;
use crate::danmaku::{LiveDanmaku, parse_bilibili_live_room_id, spawn_bilibili_live_danmaku};

mod imp {
    use std::cell::{OnceCell, RefCell};

    use adw::prelude::*;
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/mutsumi-live/ui/window.ui")]
    pub struct MutsumiLiveWindow {
        #[template_child]
        pub player: TemplateChild<MutsumiPlayer>,
        pub playlist: OnceCell<PlayList>,
        pub danmaku_stop: RefCell<Option<Arc<AtomicBool>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MutsumiLiveWindow {
        const NAME: &'static str = "MutsumiLiveWindow";
        type Type = super::MutsumiLiveWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            MutsumiPlayer::ensure_type();
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MutsumiLiveWindow {
        fn constructed(&self) {
            self.parent_constructed();

            let playlist = PlayList::new();
            self.player.playlist_bin().set_child(Some(&playlist));
            self.player.playlist_stack_page().set_visible(true);
            self.player.mpv().set_property(
                "ytdl-raw-options",
                "cookies-from-browser=firefox".to_string(),
            );
            // self.player.overlay().add_overlay();

            let obj = self.obj();
            playlist.connect_play_requested(glib::clone!(
                #[weak]
                obj,
                move |_, name, url| {
                    let imp = obj.imp();

                    // Stop any running live danmaku task
                    if let Some(stop) = imp.danmaku_stop.take() {
                        stop.store(false, Ordering::Relaxed);
                    }

                    let params = PlayParams::builder(PlaySource::Url(url.to_owned()))
                        .title(name.to_owned())
                        .subtitle(url.to_owned())
                        .build();
                    imp.player.play(&params);

                    if let Some(room_id) = parse_bilibili_live_room_id(&url) {
                        // Clear danmaku loaded from any previous non-live video
                        imp.player.danmakw().load_danmaku(vec![]);

                        let stop = Arc::new(AtomicBool::new(true));
                        let (tx, rx) = flume::unbounded::<LiveDanmaku>();

                        spawn_bilibili_live_danmaku(room_id, tx, Arc::clone(&stop));

                        imp.danmaku_stop.replace(Some(stop));

                        let danmakw = imp.player.danmakw();
                        glib::spawn_future_local(async move {
                            while let Ok(dm) = rx.recv_async().await {
                                let color = Color {
                                    r: ((dm.color >> 16) & 0xFF) as u8,
                                    g: ((dm.color >> 8) & 0xFF) as u8,
                                    b: (dm.color & 0xFF) as u8,
                                    a: 255,
                                };
                                danmakw.add_danmaku_full(&dm.text, color, DanmakuMode::Scroll);
                            }
                        });
                    }
                }
            ));

            self.playlist.set(playlist).unwrap();
        }

        fn dispose(&self) {
            if let Some(stop) = self.danmaku_stop.take() {
                stop.store(false, Ordering::Relaxed);
            }
        }
    }

    impl WidgetImpl for MutsumiLiveWindow {}
    impl WindowImpl for MutsumiLiveWindow {}
    impl ApplicationWindowImpl for MutsumiLiveWindow {}
    impl AdwApplicationWindowImpl for MutsumiLiveWindow {}
}

glib::wrapper! {
    pub struct MutsumiLiveWindow(ObjectSubclass<imp::MutsumiLiveWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::Native, gtk::Root, gtk::ShortcutManager,
                    gtk::gio::ActionGroup, gtk::gio::ActionMap;
}

impl MutsumiLiveWindow {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    pub fn player(&self) -> MutsumiPlayer {
        self.imp().player.get()
    }
}
