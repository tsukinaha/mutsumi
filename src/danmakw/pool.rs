use crate::{Color, Danmaku, DanmakuMode};
use gtk::{gdk::FrameClock, glib, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

mod imp {
    use crate::{DanmakuClock, DanmakwRenderer, DanmakwSnapshotExt};

    use super::*;
    use gtk::TickCallbackId;

    #[derive(glib::Properties)]
    #[properties(wrapper_type = super::Danmakw)]
    pub struct Danmakw {
        #[property(get, set, default_value = 1.0)]
        pub speed_factor: Cell<f32>,

        pub renderer: RefCell<DanmakwRenderer>,
        pub clock: RefCell<Option<DanmakuClock>>,
        pub tick_callback_id: RefCell<Option<TickCallbackId>>,
    }

    impl Default for Danmakw {
        fn default() -> Self {
            Self {
                speed_factor: Cell::new(1.0),
                renderer: Default::default(),
                clock: Default::default(),
                tick_callback_id: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Danmakw {
        const NAME: &'static str = "Danmakw";
        type Type = super::Danmakw;
        type ParentType = gtk::Widget;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Danmakw {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for Danmakw {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let mut renderer = self.renderer.borrow_mut();
            snapshot.render_danmakw(&mut renderer, width, height);
        }
    }

    impl Danmakw {
        pub fn start_clock(&self) {
            let mut clock = self.clock.borrow_mut();
            if let Some(c) = clock.as_mut() {
                c.resume();
            } else {
                *clock = Some(DanmakuClock::new(self.obj().speed_factor() as f64));
            }
        }

        pub fn pause_clock(&self) {
            if let Some(clock) = self.clock.borrow_mut().as_mut() {
                clock.pause();
            }
        }
    }
}

glib::wrapper! {
    pub struct Danmakw(ObjectSubclass<imp::Danmakw>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Danmakw {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn add_danmaku(&self, text: &str) {
        self.add_danmaku_full(text, Color::default(), DanmakuMode::Scroll);
    }

    pub fn add_danmaku_full(&self, text: &str, color: Color, mode: DanmakuMode) {
        let width = self.width() as f32;
        let danmaku = Danmaku {
            content: text.to_string(),
            start: 0.0,
            color,
            mode,
        };
        self.imp()
            .renderer
            .borrow_mut()
            .add_danmaku(&self.pango_context(), width, danmaku);
    }

    pub fn load_danmaku(&self, danmaku: Vec<Danmaku>) {
        let mut renderer = self.imp()
            .renderer
            .borrow_mut();
        renderer.danmaku_queue.init(danmaku, 0.0);
        renderer.clear_danmaku();
    }

    pub fn start_rendering(&self) {
        self.start_clock();
        let id = self.add_tick_callback(Self::cb);
        self.imp().tick_callback_id.replace(Some(id));
    }

    pub fn stop_rendering(&self) {
        if let Some(id) = self.imp().tick_callback_id.borrow_mut().take() {
            id.remove();
        }
    }

    pub fn start_clock(&self) {
        self.imp().start_clock();
    }

    pub fn pause_clock(&self) {
        self.imp().pause_clock();
    }

    pub fn set_paused(&self, paused: bool) {
        if paused {
            self.pause_clock();
            self.stop_rendering();
        } else {
            self.start_rendering();
        }
    }

    pub fn update(&self, time_milis: f64) {
        let imp = self.imp();
        let width = self.width() as f32;
        imp.renderer
            .borrow_mut()
            .update(&self.pango_context(), width, time_milis);
    }

    pub fn preroll_seek(&self, time_milis: f64) {
        self.imp().clock.borrow_mut().as_mut().map(|c| c.seek(time_milis));
        self.imp().renderer.borrow_mut().rebuild_visible_state_at(
            &self.pango_context(),
            self.width() as f32,
            time_milis,
        );
    }

    fn cb(&self, _frame_clock: &FrameClock) -> glib::ControlFlow {
        let imp = self.imp();
        let width = self.width() as f32;

        let clock = imp.clock.borrow();

        let Some(clock) = clock.as_ref() else {
            return glib::ControlFlow::Continue;
        };

        imp.renderer
            .borrow_mut()
            .update(&self.pango_context(), width, clock.time_milis());

        self.queue_draw();
        glib::ControlFlow::Continue
    }
}
