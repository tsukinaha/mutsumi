use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};
use mutsumi::{MutsumiPlayer, PlayParams, PlaySource};

use crate::PlayList;

mod imp {
    use std::cell::OnceCell;

    use adw::prelude::*;
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/mutsumi-live/ui/window.ui")]
    pub struct MutsumiLiveWindow {
        #[template_child]
        pub player: TemplateChild<MutsumiPlayer>,
        pub playlist: OnceCell<PlayList>,
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

            let obj = self.obj();
            playlist.connect_play_requested(glib::clone!(
                #[weak]
                obj,
                move |_, name, url| {
                    let params = PlayParams::builder(PlaySource::Url(url.to_owned()))
                        .title(name.to_owned())
                        .subtitle(url.to_owned())
                        .build();
                    obj.imp().player.play(&params);
                }
            ));

            self.playlist.set(playlist).unwrap();
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
