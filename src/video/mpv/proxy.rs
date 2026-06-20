use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    rc::Rc,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;

use wl_proxy::protocols::{
    ObjectInterface,
    fractional_scale_v1::{
        wp_fractional_scale_manager_v1::{
            WpFractionalScaleManagerV1, WpFractionalScaleManagerV1Handler,
        },
        wp_fractional_scale_v1::{WpFractionalScaleV1, WpFractionalScaleV1Handler},
    },
    linux_dmabuf_v1::{
        zwp_linux_buffer_params_v1::{
            ZwpLinuxBufferParamsV1, ZwpLinuxBufferParamsV1Flags, ZwpLinuxBufferParamsV1Handler,
        },
        zwp_linux_dmabuf_feedback_v1::{
            ZwpLinuxDmabufFeedbackV1, ZwpLinuxDmabufFeedbackV1Handler,
            ZwpLinuxDmabufFeedbackV1TrancheFlags,
        },
        zwp_linux_dmabuf_v1::{ZwpLinuxDmabufV1, ZwpLinuxDmabufV1Handler},
    },
    viewporter::{
        wp_viewport::{WpViewport, WpViewportHandler},
        wp_viewporter::{WpViewporter, WpViewporterHandler},
    },
    wayland::{
        wl_buffer::{WlBuffer, WlBufferHandler},
        wl_callback::WlCallback,
        wl_compositor::{WlCompositor, WlCompositorHandler},
        wl_display::{WlDisplay, WlDisplayHandler},
        wl_registry::{WlRegistry, WlRegistryHandler},
        wl_subcompositor::{WlSubcompositor, WlSubcompositorHandler},
        wl_subsurface::{WlSubsurface, WlSubsurfaceHandler},
        wl_surface::{WlSurface, WlSurfaceHandler},
    },
    xdg_shell::{
        xdg_surface::{XdgSurface, XdgSurfaceHandler},
        xdg_toplevel::{XdgToplevel, XdgToplevelHandler},
        xdg_wm_base::{XdgWmBase, XdgWmBaseHandler},
    },
};
use wl_proxy::{
    baseline::Baseline,
    client::ClientHandler,
    global_mapper::GlobalMapper,
    object::{Object, ObjectCoreApi, ObjectRcUtils},
    state::{Destructor, State},
};

pub struct DmabufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

pub struct DmabufFrame {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub planes: Vec<DmabufPlane>,
}

pub static FRAME_CHANNEL: Lazy<DmabufFrameChannel> = Lazy::new(|| {
    let (tx, rx) = flume::unbounded::<DmabufFrame>();
    DmabufFrameChannel { tx, rx }
});

pub struct DmabufFrameChannel {
    pub tx: flume::Sender<DmabufFrame>,
    pub rx: flume::Receiver<DmabufFrame>,
}

pub static VIEWPORT_CHANNEL: Lazy<ViewportChannel> = Lazy::new(|| {
    let (tx, rx) = flume::unbounded::<(i32, i32, f64)>();
    ViewportChannel { tx, rx }
});

pub struct ViewportChannel {
    pub tx: flume::Sender<(i32, i32, f64)>,
    pub rx: flume::Receiver<(i32, i32, f64)>,
}

static CURRENT_SCALE: Mutex<f64> = Mutex::new(1.0);

struct StoredPlane {
    fd: OwnedFd,
    offset: u32,
    stride: u32,
}

struct BufferInfo {
    _buffer: Rc<WlBuffer>,
    planes: Vec<StoredPlane>,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
}

impl BufferInfo {
    fn to_frame(&self) -> DmabufFrame {
        let planes = self
            .planes
            .iter()
            .map(|p| {
                let raw = unsafe { libc::dup(p.fd.as_raw_fd()) };
                DmabufPlane {
                    fd: unsafe { OwnedFd::from_raw_fd(raw) },
                    offset: p.offset,
                    stride: p.stride,
                }
            })
            .collect();

        DmabufFrame {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            planes,
        }
    }
}

struct ToplevelEntry {
    xdg_surface: Rc<XdgSurface>,
    toplevel: Rc<XdgToplevel>,
}

struct SharedState {
    buffer_info: HashMap<u64, BufferInfo>,
    toplevels: Vec<ToplevelEntry>,
    configure_serial: u32,
    fractional_scales: Vec<Rc<WpFractionalScaleV1>>,
}

impl SharedState {
    fn configure_toplevels(&mut self, width: i32, height: i32) {
        for entry in &self.toplevels {
            entry.toplevel.send_configure(width, height, &[]);
            entry.xdg_surface.send_configure(self.configure_serial);
            self.configure_serial = self.configure_serial.wrapping_add(1);
        }
    }

    fn update_fractional_scales(&mut self, scale_120: u32) {
        for s in &self.fractional_scales {
            s.send_preferred_scale(scale_120);
        }
    }
}

fn current_time_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

struct DisplayHandler {
    state: Rc<RefCell<SharedState>>,
}

impl WlDisplayHandler for DisplayHandler {
    fn handle_get_registry(&mut self, slf: &Rc<WlDisplay>, registry: &Rc<WlRegistry>) {
        slf.send_get_registry(registry);

        let mut mapper = GlobalMapper::default();

        let xdg_wm_base_client_name =
            mapper.add_synthetic_global(registry, ObjectInterface::XdgWmBase, 4);
        let viewporter_client_name =
            mapper.add_synthetic_global(registry, ObjectInterface::WpViewporter, 1);
        let fractional_scale_manager_client_name =
            mapper.add_synthetic_global(registry, ObjectInterface::WpFractionalScaleManagerV1, 1);

        registry.set_handler(RegistryHandler {
            mapper,
            state: Rc::clone(&self.state),
            xdg_wm_base_client_name,
            viewporter_client_name,
            fractional_scale_manager_client_name,
        });
    }
}

struct RegistryHandler {
    mapper: GlobalMapper,
    state: Rc<RefCell<SharedState>>,
    xdg_wm_base_client_name: u32,
    viewporter_client_name: u32,
    fractional_scale_manager_client_name: u32,
}

impl WlRegistryHandler for RegistryHandler {
    fn handle_global(
        &mut self,
        slf: &Rc<WlRegistry>,
        name: u32,
        interface: ObjectInterface,
        version: u32,
    ) {
        if interface == ObjectInterface::XdgWmBase
            || interface == ObjectInterface::WpViewporter
            || interface == ObjectInterface::WpFractionalScaleManagerV1
        {
            self.mapper.ignore_global(name);
        } else if interface == ObjectInterface::ZwpLinuxDmabufV1 {
            self.mapper
                .forward_global(slf, name, interface, version.min(4));
        } else {
            self.mapper.forward_global(slf, name, interface, version);
        }
    }

    fn handle_global_remove(&mut self, slf: &Rc<WlRegistry>, name: u32) {
        self.mapper.forward_global_remove(slf, name);
    }

    fn handle_bind(&mut self, slf: &Rc<WlRegistry>, name: u32, id: Rc<dyn Object>) {
        if name == self.xdg_wm_base_client_name {
            let wm_base = id.downcast::<XdgWmBase>();
            wm_base.set_forward_to_server(false);
            wm_base.set_handler(WmBaseHandler {
                state: Rc::clone(&self.state),
            });
        } else if name == self.viewporter_client_name {
            let viewporter = id.downcast::<WpViewporter>();
            viewporter.set_forward_to_server(false);
            viewporter.set_handler(ViewporterHandler);
        } else if name == self.fractional_scale_manager_client_name {
            let manager = id.downcast::<WpFractionalScaleManagerV1>();
            manager.set_forward_to_server(false);
            manager.set_handler(FractionalScaleManagerHandler {
                state: Rc::clone(&self.state),
            });
        } else {
            let compositor = id.try_downcast::<WlCompositor>();
            let subcompositor = id.try_downcast::<WlSubcompositor>();
            let dmabuf = id.try_downcast::<ZwpLinuxDmabufV1>();

            self.mapper.forward_bind(slf, name, &id);

            if let Some(compositor) = compositor {
                compositor.set_handler(CompositorHandler {
                    state: Rc::clone(&self.state),
                });
            } else if let Some(subcompositor) = subcompositor {
                subcompositor.set_handler(SubcompositorHandler);
            } else if let Some(dmabuf) = dmabuf {
                dmabuf.set_handler(DmabufHandler {
                    state: Rc::clone(&self.state),
                });
            }
        }
    }
}

struct CompositorHandler {
    state: Rc<RefCell<SharedState>>,
}

impl WlCompositorHandler for CompositorHandler {
    fn handle_create_surface(&mut self, _slf: &Rc<WlCompositor>, id: &Rc<WlSurface>) {
        id.set_forward_to_server(false);
        id.set_handler(SurfaceHandler {
            shared: Rc::clone(&self.state),
            pending_buffer: None,
            pending_callbacks: Vec::new(),
        });
    }
}

struct SubcompositorHandler;

impl WlSubcompositorHandler for SubcompositorHandler {
    fn handle_get_subsurface(
        &mut self,
        _slf: &Rc<WlSubcompositor>,
        id: &Rc<WlSubsurface>,
        _surface: &Rc<WlSurface>,
        _parent: &Rc<WlSurface>,
    ) {
        id.set_forward_to_server(false);
        id.set_handler(SubsurfaceHandler);
    }
}

struct SubsurfaceHandler;

impl WlSubsurfaceHandler for SubsurfaceHandler {
    fn handle_destroy(&mut self, slf: &Rc<WlSubsurface>) {
        slf.delete_id();
    }
}

struct ViewporterHandler;

impl WpViewporterHandler for ViewporterHandler {
    fn handle_destroy(&mut self, slf: &Rc<WpViewporter>) {
        slf.delete_id();
    }

    fn handle_get_viewport(
        &mut self,
        _slf: &Rc<WpViewporter>,
        id: &Rc<WpViewport>,
        _surface: &Rc<WlSurface>,
    ) {
        id.set_forward_to_server(false);
        id.set_handler(ViewportHandler);
    }
}

struct ViewportHandler;

impl WpViewportHandler for ViewportHandler {
    fn handle_destroy(&mut self, slf: &Rc<WpViewport>) {
        slf.delete_id();
    }
}

struct FractionalScaleManagerHandler {
    state: Rc<RefCell<SharedState>>,
}

impl WpFractionalScaleManagerV1Handler for FractionalScaleManagerHandler {
    fn handle_destroy(&mut self, slf: &Rc<WpFractionalScaleManagerV1>) {
        slf.delete_id();
    }

    fn handle_get_fractional_scale(
        &mut self,
        _slf: &Rc<WpFractionalScaleManagerV1>,
        id: &Rc<WpFractionalScaleV1>,
        _surface: &Rc<WlSurface>,
    ) {
        id.set_forward_to_server(false);
        let scale_120 = (*CURRENT_SCALE.lock().unwrap() * 120.0).round() as u32;
        id.send_preferred_scale(scale_120);
        id.set_handler(FractionalScaleHandler {
            state: Rc::clone(&self.state),
        });
        self.state
            .borrow_mut()
            .fractional_scales
            .push(Rc::clone(id));
    }
}

struct FractionalScaleHandler {
    state: Rc<RefCell<SharedState>>,
}

impl WpFractionalScaleV1Handler for FractionalScaleHandler {
    fn handle_destroy(&mut self, slf: &Rc<WpFractionalScaleV1>) {
        self.state
            .borrow_mut()
            .fractional_scales
            .retain(|s| !Rc::ptr_eq(s, slf));
        slf.delete_id();
    }
}

struct SurfaceHandler {
    shared: Rc<RefCell<SharedState>>,
    pending_buffer: Option<Rc<WlBuffer>>,
    pending_callbacks: Vec<Rc<WlCallback>>,
}

impl WlSurfaceHandler for SurfaceHandler {
    fn handle_destroy(&mut self, slf: &Rc<WlSurface>) {
        slf.delete_id();
    }

    fn handle_attach(
        &mut self,
        _slf: &Rc<WlSurface>,
        buffer: Option<&Rc<WlBuffer>>,
        _x: i32,
        _y: i32,
    ) {
        self.pending_buffer = buffer.map(Rc::clone);
    }

    fn handle_frame(&mut self, _slf: &Rc<WlSurface>, callback: &Rc<WlCallback>) {
        self.pending_callbacks.push(Rc::clone(callback));
    }

    fn handle_commit(&mut self, _slf: &Rc<WlSurface>) {
        let time_ms = current_time_ms();

        for cb in std::mem::take(&mut self.pending_callbacks) {
            cb.send_done(time_ms);
            cb.delete_id();
        }

        if let Some(buffer) = self.pending_buffer.take() {
            let state = self.shared.borrow();
            if let Some(info) = state.buffer_info.get(&buffer.unique_id()) {
                let frame = info.to_frame();
                let _ = FRAME_CHANNEL.tx.send(frame);
            }
            buffer.send_release();
        }
    }
}

static ALLOWED_FORMAT_PAIRS: OnceLock<HashSet<(u32, u64)>> = OnceLock::new();

struct DmabufHandler {
    state: Rc<RefCell<SharedState>>,
}

impl ZwpLinuxDmabufV1Handler for DmabufHandler {
    fn handle_format(&mut self, slf: &Rc<ZwpLinuxDmabufV1>, format: u32) {
        let allowed = ALLOWED_FORMAT_PAIRS.get();
        if allowed.is_some_and(|pairs| pairs.iter().any(|(f, _)| *f == format)) {
            slf.send_format(format);
        }
    }

    fn handle_modifier(
        &mut self,
        slf: &Rc<ZwpLinuxDmabufV1>,
        format: u32,
        modifier_hi: u32,
        modifier_lo: u32,
    ) {
        let modifier = ((modifier_hi as u64) << 32) | (modifier_lo as u64);
        let allowed = ALLOWED_FORMAT_PAIRS.get();
        if allowed.is_some_and(|pairs| pairs.contains(&(format, modifier))) {
            slf.send_modifier(format, modifier_hi, modifier_lo);
        }
    }

    fn handle_create_params(
        &mut self,
        _slf: &Rc<ZwpLinuxDmabufV1>,
        params_id: &Rc<ZwpLinuxBufferParamsV1>,
    ) {
        params_id.set_forward_to_server(false);
        params_id.set_handler(BufferParamsHandler {
            state: Rc::clone(&self.state),
            planes: Vec::new(),
            modifier: 0,
        });
    }

    fn handle_get_default_feedback(
        &mut self,
        slf: &Rc<ZwpLinuxDmabufV1>,
        id: &Rc<ZwpLinuxDmabufFeedbackV1>,
    ) {
        let allowed = ALLOWED_FORMAT_PAIRS.get().cloned().unwrap_or_default();
        id.set_handler(FeedbackHandler {
            allowed,
            index_map: Vec::new(),
            pending_device: None,
            pending_flags: None,
            pending_formats: Vec::new(),
        });
        slf.send_get_default_feedback(id);
    }

    fn handle_get_surface_feedback(
        &mut self,
        slf: &Rc<ZwpLinuxDmabufV1>,
        id: &Rc<ZwpLinuxDmabufFeedbackV1>,
        _surface: &Rc<WlSurface>,
    ) {
        let allowed = ALLOWED_FORMAT_PAIRS.get().cloned().unwrap_or_default();
        id.set_handler(FeedbackHandler {
            allowed,
            index_map: Vec::new(),
            pending_device: None,
            pending_flags: None,
            pending_formats: Vec::new(),
        });
        slf.send_get_default_feedback(id);
    }
}

struct FeedbackHandler {
    allowed: HashSet<(u32, u64)>,
    index_map: Vec<Option<u16>>,
    pending_device: Option<Vec<u8>>,
    pending_flags: Option<ZwpLinuxDmabufFeedbackV1TrancheFlags>,
    pending_formats: Vec<u16>,
}

impl ZwpLinuxDmabufFeedbackV1Handler for FeedbackHandler {
    fn handle_format_table(
        &mut self,
        slf: &Rc<ZwpLinuxDmabufFeedbackV1>,
        fd: &Rc<OwnedFd>,
        size: u32,
    ) {
        let num_entries = size as usize / 16;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size as usize,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            slf.send_format_table(fd, size);
            return;
        }

        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size as usize) };
        let mut new_table: Vec<u8> = Vec::new();
        self.index_map = vec![None; num_entries];
        let mut new_index: u16 = 0;

        for i in 0..num_entries {
            let base = i * 16;
            let format = u32::from_ne_bytes(bytes[base..base + 4].try_into().unwrap());
            let modifier = u64::from_ne_bytes(bytes[base + 8..base + 16].try_into().unwrap());
            if self.allowed.contains(&(format, modifier)) {
                self.index_map[i] = Some(new_index);
                new_index = new_index.saturating_add(1);
                new_table.extend_from_slice(&bytes[base..base + 16]);
            }
        }

        unsafe { libc::munmap(ptr, size as usize) };

        let memfd = unsafe { libc::memfd_create(c"dmabuf-fb".as_ptr() as *const libc::c_char, 0) };
        if memfd < 0 {
            return;
        }
        unsafe {
            libc::write(
                memfd,
                new_table.as_ptr() as *const libc::c_void,
                new_table.len(),
            )
        };
        let new_fd = Rc::new(unsafe { OwnedFd::from_raw_fd(memfd) });
        slf.send_format_table(&new_fd, new_table.len() as u32);
    }

    fn handle_main_device(&mut self, slf: &Rc<ZwpLinuxDmabufFeedbackV1>, device: &[u8]) {
        slf.send_main_device(device);
    }

    fn handle_tranche_target_device(&mut self, _slf: &Rc<ZwpLinuxDmabufFeedbackV1>, device: &[u8]) {
        self.pending_device = Some(device.to_vec());
        self.pending_flags = None;
        self.pending_formats.clear();
    }

    fn handle_tranche_flags(
        &mut self,
        _slf: &Rc<ZwpLinuxDmabufFeedbackV1>,
        flags: ZwpLinuxDmabufFeedbackV1TrancheFlags,
    ) {
        self.pending_flags = Some(flags);
    }

    fn handle_tranche_formats(&mut self, _slf: &Rc<ZwpLinuxDmabufFeedbackV1>, indices: &[u8]) {
        for chunk in indices.chunks_exact(2) {
            let old = u16::from_ne_bytes([chunk[0], chunk[1]]);
            if let Some(Some(new)) = self.index_map.get(old as usize) {
                self.pending_formats.push(*new);
            }
        }
    }

    fn handle_tranche_done(&mut self, slf: &Rc<ZwpLinuxDmabufFeedbackV1>) {
        if self.pending_formats.is_empty() {
            self.pending_device = None;
            self.pending_flags = None;
            return;
        }
        if let Some(device) = self.pending_device.take() {
            slf.send_tranche_target_device(&device);
        }
        slf.send_tranche_flags(self.pending_flags.take().unwrap_or_default());
        let bytes: Vec<u8> = self
            .pending_formats
            .drain(..)
            .flat_map(|i| i.to_ne_bytes())
            .collect();
        slf.send_tranche_formats(&bytes);
        slf.send_tranche_done();
    }

    fn handle_done(&mut self, slf: &Rc<ZwpLinuxDmabufFeedbackV1>) {
        slf.send_done();
    }
}

struct BufferParamsHandler {
    state: Rc<RefCell<SharedState>>,
    planes: Vec<StoredPlane>,
    modifier: u64,
}

impl ZwpLinuxBufferParamsV1Handler for BufferParamsHandler {
    fn handle_destroy(&mut self, slf: &Rc<ZwpLinuxBufferParamsV1>) {
        slf.delete_id();
    }

    fn handle_add(
        &mut self,
        _slf: &Rc<ZwpLinuxBufferParamsV1>,
        fd: &Rc<OwnedFd>,
        _plane_idx: u32,
        offset: u32,
        stride: u32,
        modifier_hi: u32,
        modifier_lo: u32,
    ) {
        let raw = unsafe { libc::dup(fd.as_raw_fd()) };
        let dup_fd = unsafe { OwnedFd::from_raw_fd(raw) };

        self.modifier = ((modifier_hi as u64) << 32) | (modifier_lo as u64);
        self.planes.push(StoredPlane {
            fd: dup_fd,
            offset,
            stride,
        });
    }

    fn handle_create_immed(
        &mut self,
        _slf: &Rc<ZwpLinuxBufferParamsV1>,
        buffer_id: &Rc<WlBuffer>,
        width: i32,
        height: i32,
        format: u32,
        _flags: ZwpLinuxBufferParamsV1Flags,
    ) {
        buffer_id.set_forward_to_server(false);
        buffer_id.set_handler(WlBufferHandlerImpl {
            shared: Rc::clone(&self.state),
        });

        let info = BufferInfo {
            _buffer: Rc::clone(buffer_id),
            planes: std::mem::take(&mut self.planes),
            width: width as u32,
            height: height as u32,
            format,
            modifier: self.modifier,
        };

        self.state
            .borrow_mut()
            .buffer_info
            .insert(buffer_id.unique_id(), info);
    }
}

struct WlBufferHandlerImpl {
    shared: Rc<RefCell<SharedState>>,
}

impl WlBufferHandler for WlBufferHandlerImpl {
    fn handle_destroy(&mut self, slf: &Rc<WlBuffer>) {
        self.shared
            .borrow_mut()
            .buffer_info
            .remove(&slf.unique_id());

        slf.delete_id();
    }
}

struct WmBaseHandler {
    state: Rc<RefCell<SharedState>>,
}

impl XdgWmBaseHandler for WmBaseHandler {
    fn handle_destroy(&mut self, slf: &Rc<XdgWmBase>) {
        slf.delete_id();
    }

    fn handle_get_xdg_surface(
        &mut self,
        _slf: &Rc<XdgWmBase>,
        id: &Rc<XdgSurface>,
        _surface: &Rc<WlSurface>,
    ) {
        id.set_forward_to_server(false);
        id.set_handler(XdgSurfaceHandlerImpl {
            state: Rc::clone(&self.state),
        });
    }

    fn handle_pong(&mut self, _slf: &Rc<XdgWmBase>, _serial: u32) {}
}

struct XdgSurfaceHandlerImpl {
    state: Rc<RefCell<SharedState>>,
}

impl XdgSurfaceHandler for XdgSurfaceHandlerImpl {
    fn handle_destroy(&mut self, slf: &Rc<XdgSurface>) {
        let surface_id = slf.unique_id();
        self.state
            .borrow_mut()
            .toplevels
            .retain(|e| e.xdg_surface.unique_id() != surface_id);
        slf.delete_id();
    }

    fn handle_get_toplevel(&mut self, slf: &Rc<XdgSurface>, id: &Rc<XdgToplevel>) {
        id.set_forward_to_server(false);
        id.set_handler(XdgToplevelHandlerImpl {
            state: Rc::clone(&self.state),
        });

        id.send_configure_bounds(0, 0);

        let mut state = self.state.borrow_mut();
        let serial = state.configure_serial;
        state.configure_serial = serial.wrapping_add(1);
        slf.send_configure(serial);

        state.toplevels.push(ToplevelEntry {
            xdg_surface: Rc::clone(slf),
            toplevel: Rc::clone(id),
        });
    }

    fn handle_ack_configure(&mut self, _slf: &Rc<XdgSurface>, _serial: u32) {}
}

struct XdgToplevelHandlerImpl {
    state: Rc<RefCell<SharedState>>,
}

impl XdgToplevelHandler for XdgToplevelHandlerImpl {
    fn handle_destroy(&mut self, slf: &Rc<XdgToplevel>) {
        let toplevel_id = slf.unique_id();
        self.state
            .borrow_mut()
            .toplevels
            .retain(|e| e.toplevel.unique_id() != toplevel_id);
        slf.delete_id();
    }
}

struct ClientHandlerImpl {
    _destructor: Destructor,
}

impl ClientHandler for ClientHandlerImpl {
    fn disconnected(self: Box<Self>) {
        tracing::debug!("wl-proxy-mpv: client disconnected");
    }
}

fn serve_client(socket: OwnedFd, upstream: String) {
    let state = match State::builder(Baseline::ALL_OF_THEM)
        .with_server_display_name(&upstream)
        .build()
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("wl-proxy-mpv: failed to create state: {e}");
            return;
        }
    };
    let client = match state.add_client(&Rc::new(socket)) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("wl-proxy-mpv: failed to add client: {e}");
            return;
        }
    };
    client.set_handler(ClientHandlerImpl {
        _destructor: state.create_destructor(),
    });

    let shared = Rc::new(RefCell::new(SharedState {
        buffer_info: HashMap::new(),
        toplevels: Vec::new(),
        configure_serial: 1,
        fractional_scales: Vec::new(),
    }));
    client.display().set_handler(DisplayHandler {
        state: Rc::clone(&shared),
    });

    while state.is_not_destroyed() {
        if let Err(e) = state.dispatch(Some(Duration::from_millis(50))) {
            tracing::error!("wl-proxy-mpv: dispatch failed: {e}");
            return;
        }

        let mut latest = None;
        while let Ok(viewport) = VIEWPORT_CHANNEL.rx.try_recv() {
            latest = Some(viewport);
        }
        if let Some((width, height, scale)) = latest {
            shared.borrow_mut().configure_toplevels(width, height);
            *CURRENT_SCALE.lock().unwrap() = scale;
            let scale_120 = (scale * 120.0).round() as u32;
            shared.borrow_mut().update_fractional_scales(scale_120);
        }
    }
}

static PROXY_ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn create_mpv_proxy(format_pairs: Vec<(u32, u64)>) {
    ALLOWED_FORMAT_PAIRS
        .set(format_pairs.into_iter().collect())
        .ok();
}

pub fn arm_mpv_proxy() {
    use std::sync::atomic::Ordering;

    if PROXY_ARMED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let upstream = match std::env::var("WAYLAND_DISPLAY") {
        Ok(s) => s,
        Err(_) => {
            PROXY_ARMED.store(false, Ordering::SeqCst);
            return;
        }
    };

    let Ok((client, server)) = std::os::unix::net::UnixStream::pair() else {
        PROXY_ARMED.store(false, Ordering::SeqCst);
        return;
    };

    let result = std::thread::Builder::new()
        .name("wl-proxy-mpv".into())
        .spawn(move || {
            serve_client(server.into(), upstream);
            PROXY_ARMED.store(false, Ordering::SeqCst);
        });

    if result.is_err() {
        PROXY_ARMED.store(false, Ordering::SeqCst);
        return;
    }

    unsafe { std::env::set_var("WAYLAND_SOCKET", client.into_raw_fd().to_string()) };
}
