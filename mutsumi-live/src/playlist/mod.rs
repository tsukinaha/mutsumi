mod source_row;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib};
use serde::{Deserialize, Serialize};

pub use source_row::SourceActionRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntry {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Config {
    #[serde(default)]
    sources: Vec<SourceEntry>,
}

fn config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("mutsumi-live.yaml"))
}

fn load_config() -> Config {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(config: &Config) {
    let Some(path) = config_path() else { return };
    if let Ok(yaml) = serde_yaml::to_string(config) {
        let _ = std::fs::write(path, yaml);
    }
}

mod imp {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    use adw::prelude::*;
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/mutsumi-live/ui/playlist.ui")]
    #[properties(wrapper_type = super::PlayList)]
    pub struct PlayList {
        #[template_child]
        pub bottom_sheet: TemplateChild<adw::BottomSheet>,
        #[template_child]
        pub listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub name_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub url_entry: TemplateChild<adw::EntryRow>,

        pub sources: RefCell<Vec<super::SourceEntry>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PlayList {
        const NAME: &'static str = "PlayList";
        type Type = super::PlayList;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            super::SourceActionRow::ensure_type();
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PlayList {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    glib::subclass::Signal::builder("play-requested")
                        .param_types([String::static_type(), String::static_type()])
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();

            let config = super::load_config();
            for entry in &config.sources {
                self.append_row(entry);
            }
            *self.sources.borrow_mut() = config.sources;
        }
    }

    impl PlayList {
        pub fn append_row(&self, entry: &super::SourceEntry) {
            let row = super::SourceActionRow::new(&entry.name, &entry.url);
            self.listbox.append(&row);
            row.refresh_live_status();

            let obj = self.obj();
            row.connect_delete_requested(glib::clone!(
                #[weak]
                obj,
                #[weak]
                row,
                move |_| {
                    let imp = obj.imp();
                    let idx = row.index() as usize;
                    imp.sources.borrow_mut().remove(idx);
                    super::save_config(&super::Config {
                        sources: imp.sources.borrow().clone(),
                    });
                    imp.listbox.remove(&row);
                }
            ));
        }
    }

    #[gtk::template_callbacks]
    impl PlayList {
        #[template_callback]
        fn on_add_activated(&self) {
            let name = self.name_entry.text().to_string();
            let url = self.url_entry.text().to_string();

            if name.is_empty() || url.is_empty() {
                return;
            }

            let entry = super::SourceEntry { name, url };
            self.append_row(&entry);
            self.sources.borrow_mut().push(entry);

            super::save_config(&super::Config {
                sources: self.sources.borrow().clone(),
            });

            self.name_entry.set_text("");
            self.url_entry.set_text("");
            self.bottom_sheet.set_open(false);
        }

        #[template_callback]
        fn on_refresh_activated(&self) {
            let mut child = self.listbox.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Ok(row) = widget.downcast::<super::SourceActionRow>() {
                    row.refresh_live_status();
                }
            }
        }

        #[template_callback]
        fn on_row_activated(&self, row: &gtk::ListBoxRow) {
            let idx = row.index() as usize;
            let sources = self.sources.borrow();
            let Some(entry) = sources.get(idx) else {
                return;
            };
            self.obj()
                .emit_by_name::<()>("play-requested", &[&entry.name, &entry.url]);
        }
    }

    impl WidgetImpl for PlayList {}
    impl BinImpl for PlayList {}
}

glib::wrapper! {
    pub struct PlayList(ObjectSubclass<imp::PlayList>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PlayList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn connect_play_requested<F: Fn(&Self, &str, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "play-requested",
            false,
            glib::closure_local!(move |obj: Self, name: String, url: String| {
                f(&obj, &name, &url);
            }),
        )
    }
}

impl Default for PlayList {
    fn default() -> Self {
        Self::new()
    }
}
