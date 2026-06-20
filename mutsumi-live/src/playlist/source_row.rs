use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LiveStatus {
    /// Not a recognized live source; the indicator is hidden.
    #[default]
    Unknown,
    Loading,
    Live,
    Offline,
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::OnceLock;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/mutsumi-live/ui/source_row.ui")]
    #[properties(wrapper_type = super::SourceActionRow)]
    pub struct SourceActionRow {
        #[template_child]
        pub live_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub live_icon: TemplateChild<gtk::Image>,

        #[property(get, set)]
        pub source_url: RefCell<String>,
        pub live_status: Cell<LiveStatus>,
    }

    impl SourceActionRow {
        pub fn set_live_status(&self, status: LiveStatus) {
            self.live_status.set(status);

            match status {
                LiveStatus::Unknown => {
                    self.live_icon.set_css_classes(&[]);
                    self.live_icon.set_tooltip_text(Some("Unknown"));
                    self.live_stack.set_visible_child_name("status");
                }
                LiveStatus::Loading => {
                    self.live_stack.set_visible_child_name("loading");
                }
                LiveStatus::Live => {
                    self.live_icon.set_css_classes(&["success"]);
                    self.live_icon.set_tooltip_text(Some("Live"));
                    self.live_stack.set_visible_child_name("status");
                }
                LiveStatus::Offline => {
                    self.live_icon.set_css_classes(&["error"]);
                    self.live_icon.set_tooltip_text(Some("Offline"));
                    self.live_stack.set_visible_child_name("status");
                }
            }
        }
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
            SIGNALS
                .get_or_init(|| vec![glib::subclass::Signal::builder("delete-requested").build()])
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

    pub fn refresh_live_status(&self) {
        let url = self.source_url();

        if let Some(room_id) = crate::danmaku::parse_bilibili_live_room_id(&url) {
            self.imp().set_live_status(LiveStatus::Loading);
            let weak = self.downgrade();
            glib::spawn_future_local(async move {
                let status = match crate::danmaku::check_bilibili_live_status(room_id).await {
                    Some(true) => LiveStatus::Live,
                    Some(false) => LiveStatus::Offline,
                    None => LiveStatus::Unknown,
                };
                if let Some(row) = weak.upgrade() {
                    row.imp().set_live_status(status);
                }
            });
            return;
        }

        if let Some(rid) = crate::danmaku::parse_douyu_room_id(&url) {
            self.imp().set_live_status(LiveStatus::Loading);
            let weak = self.downgrade();
            glib::spawn_future_local(async move {
                let status = match crate::danmaku::check_douyu_live_status(&rid).await {
                    Some(true) => LiveStatus::Live,
                    Some(false) => LiveStatus::Offline,
                    None => LiveStatus::Unknown,
                };
                if let Some(row) = weak.upgrade() {
                    row.imp().set_live_status(status);
                }
            });
            return;
        }

        self.imp().set_live_status(LiveStatus::Unknown);
    }
}
