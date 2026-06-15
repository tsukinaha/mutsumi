use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};

mod imp {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/mutsumi-live/ui/source_row.ui")]
    #[properties(wrapper_type = super::SourceActionRow)]
    pub struct SourceActionRow {
        #[property(get, set)]
        pub source_url: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SourceActionRow {
        const NAME: &'static str = "SourceActionRow";
        type Type = super::SourceActionRow;
        type ParentType = adw::ActionRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SourceActionRow {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![glib::subclass::Signal::builder("delete-requested").build()]
            })
        }
    }

    impl WidgetImpl for SourceActionRow {}
    impl ListBoxRowImpl for SourceActionRow {}
    impl PreferencesRowImpl for SourceActionRow {}
    impl ActionRowImpl for SourceActionRow {}

    #[gtk::template_callbacks]
    impl SourceActionRow {
        #[template_callback]
        fn on_delete_clicked(&self) {
            self.obj().emit_by_name::<()>("delete-requested", &[]);
        }
    }
}

glib::wrapper! {
    pub struct SourceActionRow(ObjectSubclass<imp::SourceActionRow>)
        @extends adw::ActionRow, adw::PreferencesRow, gtk::ListBoxRow, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl SourceActionRow {
    pub fn new(name: &str, url: &str) -> Self {
        let row: Self = glib::Object::new();
        row.set_title(name);
        row.set_subtitle(url);
        row.set_source_url(url.to_owned());
        row
    }

    pub fn connect_delete_requested<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "delete-requested",
            false,
            glib::closure_local!(move |obj: Self| f(&obj)),
        )
    }
}
