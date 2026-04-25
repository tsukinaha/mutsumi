use glib::Object;
use gtk::{gio, glib, subclass::prelude::*};
use tracing::info;


use crate::video::mpv::contexted::{ContextedMPV, TrackSelection};

use super::RENDER_UPDATE;

mod imp {
    use crate::video::{
        MPV_CTRL, MpvMessage, MutsumiMpvError, SendableFn, mpv::contexted::ContextedMPV,
    };
    use std::{ffi::c_void, sync::OnceLock};

    use super::*;

    use flume::bounded;
    #[cfg(target_os = "linux")]
    use gdk_wayland::{WaylandDisplay, wayland_client::Proxy};

    #[cfg(target_os = "linux")]
    use gdk_x11::X11Display;

    use glib::subclass::Signal;
    use glow::HasContext;
    use gtk::{
        gdk::{Display, GLContext},
        glib,
        prelude::*,
    };
    use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
    use once_cell::sync::OnceCell;

    #[derive(Default)]
    pub struct MPVGLArea {
        pub mpv: ContextedMPV,
        pub mpv_ctx: OnceCell<RenderContext>,
        pub gl_ctx: OnceCell<glow::Context>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MPVGLArea {
        const NAME: &'static str = "MPVGLArea";
        type Type = super::MPVGLArea;
        type ParentType = gtk::GLArea;
    }

    impl ObjectImpl for MPVGLArea {
        fn constructed(&self) {
            self.parent_constructed();
        }

        fn dispose(&self) {
            self.mpv.shutdown();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("mutsumi-error")
                        .param_types([glib::Type::I32])
                        .build(),
                ]
            })
        }
    }

    impl WidgetImpl for MPVGLArea {
        fn realize(&self) {
            self.parent_realize();

            let obj = self.obj();
            if obj.error().is_some() {
                self.throw_error(MutsumiMpvError::AreaNotInitialized);
                return;
            }

            obj.make_current();
            let Some(gl_context) = obj.context() else {
                self.throw_error(MutsumiMpvError::ContextNotInitialized);
                return;
            };

            self.setup_mpv(gl_context, obj.display());

            glib::spawn_future_local(glib::clone!(
                #[weak]
                obj,
                async move {
                    while RENDER_UPDATE.rx.recv_async().await.is_ok() {
                        obj.queue_render();
                    }
                }
            ));
        }

        fn unrealize(&self) {
            self.parent_unrealize();
        }
    }

    impl GLAreaImpl for MPVGLArea {
        fn render(&self, _context: &GLContext) -> glib::Propagation {
            let Some(ctx) = self.mpv_ctx.get() else {
                return glib::Propagation::Stop;
            };

            let factor = self.obj().scale_factor();
            let width = self.obj().width() * factor;
            let height = self.obj().height() * factor;

            unsafe {
                let fbo = self.glow_cxt().get_parameter_i32(glow::FRAMEBUFFER_BINDING);
                ctx.render::<GLContext>(fbo, width, height, true).ok();
            }
            glib::Propagation::Stop
        }
    }

    impl MPVGLArea {
        fn setup_mpv(&self, gl_context: GLContext, display: Display) {
            let mut render_params = vec![
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address,
                    ctx: gl_context,
                }),
            ];

            // MPV render params to enable hardware decoding on X11 and Wayland
            // displays.
            //
            // https://github.com/mpv-player/mpv/blob/86e12929aa0bbc61946d3804982acf887786a7cb/include/mpv/render_gl.h#L91
            #[cfg(target_os = "linux")]
            if let Ok(display_wrapper) = display.clone().downcast::<X11Display>() {
                render_params.push(RenderParam::X11Display(
                    unsafe { display_wrapper.xdisplay() } as *const c_void,
                ));
            } else if let Some(display_wrapper) = display.clone().downcast::<WaylandDisplay>().ok()
                && let Some(wl_display) = display_wrapper.wl_display()
            {
                render_params.push(RenderParam::WaylandDisplay(
                    wl_display.id().as_ptr() as *const c_void
                ));
            }

            let (ctx_tx, ctx_rx) = bounded::<SendableHandle>(1);

            //SAFETY: This struct is only used to send the RenderContext across a channel, and the RenderContext is never accessed from multiple threads simultaneously. The MPV render context is created and used entirely within the GLArea's render callback, which is called from the GTK main thread. The channel is only used to transfer ownership of the RenderContext from the MPV initialization closure to the GLArea instance, and there are no concurrent accesses to the RenderContext from multiple threads.
            struct SendableHandle(*mut libmpv2_sys::mpv_handle);
            unsafe impl Send for SendableHandle {}

            MPV_CTRL
                .tx
                .send(MpvMessage::InitRenderContext(SendableFn::new(
                    move |handle| {
                        _ = ctx_tx.send(SendableHandle(handle));
                    },
                )))
                .expect("Init render context failed");

            glib::spawn_future_local(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    let handle = ctx_rx.recv_async().await.expect("Actor dropped sender");
                    let mut ctx =
                        RenderContext::new(unsafe { handle.0.as_mut().expect("mpv_handle is pointed to null???") }, render_params)
                            .expect("Failed creating render context");
                    ctx.set_update_callback(|| {
                        let _ = RENDER_UPDATE.tx.send(true);
                    });
                    imp.mpv_ctx
                        .set(ctx)
                        .ok()
                        .expect("MPV render context already set???");
                }
            ));
        }

        fn glow_cxt(&self) -> &glow::Context {
            self.gl_ctx.get_or_init(|| unsafe {
                glow::Context::from_loader_function(epoxy::get_proc_addr)
            })
        }

        fn throw_error(&self, code: MutsumiMpvError) {
            self.obj().emit_by_name::<()>("mutsumi-error", &[&code]);
        }
    }

    fn get_proc_address(_ctx: &GLContext, name: &str) -> *mut c_void {
        epoxy::get_proc_addr(name) as *mut c_void
    }
}

glib::wrapper! {
    pub struct MPVGLArea(ObjectSubclass<imp::MPVGLArea>)
        @extends gtk::Widget ,gtk::GLArea,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl Default for MPVGLArea {
    fn default() -> Self {
        Self::new()
    }
}

impl MPVGLArea {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn mpv(&self) -> &ContextedMPV {
        &self.imp().mpv
    }

    pub fn play(&self, url: &str, percentage: f64) {
        let url = url.to_owned();

        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                let mpv = obj.mpv();

                info!("Now Playing: {}", url);
                mpv.load_video(&url);

                mpv.set_start(percentage);
                mpv.pause(false);
            }
        ));
    }

    pub fn add_sub(&self, url: &str) {
        self.mpv().add_sub(url)
    }

    pub fn seek_forward(&self, value: i64) {
        self.mpv().seek_forward(value)
    }

    pub fn seek_backward(&self, value: i64) {
        self.mpv().seek_backward(value)
    }

    pub fn set_position(&self, value: f64) {
        self.mpv().set_position(value)
    }

    pub async fn position(&self) -> f64 {
        self.mpv().position().await
    }

    pub fn set_aid(&self, value: TrackSelection) {
        self.mpv().set_aid(value)
    }

    pub async fn get_track_id(&self, type_: &str) -> i64 {
        self.mpv().get_track_id(type_).await
    }

    pub fn set_sid(&self, value: TrackSelection) {
        self.mpv().set_sid(value)
    }

    pub fn press_key(&self, key: u32, state: gtk::gdk::ModifierType) {
        self.mpv().press_key(key, state)
    }

    pub fn release_key(&self, key: u32, state: gtk::gdk::ModifierType) {
        self.mpv().release_key(key, state)
    }

    pub fn set_speed(&self, value: f64) {
        self.mpv().set_speed(value)
    }

    pub fn set_volume(&self, value: i64) {
        self.mpv().set_volume(value)
    }

    pub fn display_stats_toggle(&self) {
        self.mpv().display_stats_toggle()
    }

    pub async fn paused(&self) -> bool {
        self.mpv().paused().await
    }

    pub fn pause(&self) {
        self.mpv().command_pause();
    }

    pub fn volume_scroll(&self, value: i64) {
        self.mpv().volume_scroll(value)
    }

    pub fn set_slang(&self, value: String) {
        self.mpv().set_slang(value)
    }
}
