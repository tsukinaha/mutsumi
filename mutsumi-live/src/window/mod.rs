use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};
use mutsumi::{Color, DanmakuMode, MutsumiPlayer, PlayParams, PlaySource};

use crate::PlayList;
use crate::danmaku::{
    LiveDanmaku, get_douyu_stream_url, parse_bilibili_live_room_id, parse_douyu_room_id,
    spawn_bilibili_live_danmaku, spawn_douyu_live_danmaku,
};

mod imp {
    use std::cell::{OnceCell, RefCell};

    use adw::prelude::*;
    use glib::subclass::InitializingObject;

    use crate::status::PlaceHolderStatus;

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

            let obj = self.obj();

            let place_holder_status = PlaceHolderStatus::new();
            place_holder_status.connect_button_clicked(glib::clone!(
                #[weak]
                obj,
                move || {
                    obj.player().open_playlist();
                }
            ));
            self.player
                .overlay_status()
                .set_child(Some(&place_holder_status));

            playlist.connect_play_requested(glib::clone!(
                #[weak]
                obj,
                move |_, name, url| {
                    let imp = obj.imp();

                    // Stop any running live danmaku task
                    if let Some(stop) = imp.danmaku_stop.take() {
                        stop.store(false, Ordering::Relaxed);
                    }

                    if let Some(rid) = parse_douyu_room_id(&url) {
                        imp.player.danmakw().load_danmaku(vec![]);

                        let (stream_tx, stream_rx) =
                            flume::bounded::<Result<(String, String), String>>(1);
                        let rid_thread = rid.clone();
                        std::thread::spawn(move || {
                            tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                                .unwrap()
                                .block_on(async move {
                                    let result = get_douyu_stream_url(&rid_thread)
                                        .await
                                        .map_err(|e| e.to_string());
                                    let _ = stream_tx.send(result);
                                });
                        });

                        let weak = obj.downgrade();
                        let name = name.to_string();
                        let url = url.to_string();
                        glib::spawn_future_local(async move {
                            let (stream_url, real_rid) = match stream_rx.recv_async().await {
                                Ok(Ok(v)) => v,
                                Ok(Err(e)) => {
                                    tracing::error!(
                                        "failed to resolve douyu stream for room {rid}: {e}"
                                    );
                                    return;
                                }
                                Err(_) => return,
                            };

                            let Some(obj) = weak.upgrade() else { return };
                            let imp = obj.imp();

                            let params = PlayParams::builder(PlaySource::Url(stream_url))
                                .title(name)
                                .subtitle(url)
                                .build();
                            imp.player.play(&params);

                            let stop = Arc::new(AtomicBool::new(true));
                            let (dm_tx, dm_rx) = flume::unbounded::<LiveDanmaku>();
                            spawn_douyu_live_danmaku(real_rid, dm_tx, Arc::clone(&stop));
                            imp.danmaku_stop.replace(Some(stop));

                            let danmakw = imp.player.danmakw();
                            glib::spawn_future_local(async move {
                                while let Ok(dm) = dm_rx.recv_async().await {
                                    let color = Color {
                                        r: ((dm.color >> 16) & 0xFF) as u8,
                                        g: ((dm.color >> 8) & 0xFF) as u8,
                                        b: (dm.color & 0xFF) as u8,
                                        a: 255,
                                    };
                                    danmakw.add_danmaku_full(&dm.text, color, DanmakuMode::Scroll);
                                }
                            });
                        });
                        return;
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
