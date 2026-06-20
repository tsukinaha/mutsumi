use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};

mod imp {
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/mutsumi-live/ui/status.ui")]
    pub struct PlaceHolderStatus {
        #[template_child]
        pub button: TemplateChild<gtk::Button>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PlaceHolderStatus {
        const NAME: &'static str = "PlaceHolderStatus";
        type Type = super::PlaceHolderStatus;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PlaceHolderStatus {}

    impl WidgetImpl for PlaceHolderStatus {}
    impl BinImpl for PlaceHolderStatus {}
}

glib::wrapper! {
    pub struct PlaceHolderStatus(ObjectSubclass<imp::PlaceHolderStatus>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PlaceHolderStatus {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn connect_button_clicked<F: Fn() + 'static>(&self, f: F) {
        self.imp().button.connect_clicked(move |_| {
            f();
        });
    }
}
