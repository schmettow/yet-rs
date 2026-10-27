// SPDX-License-Identifier: GPL-3.0-only

//! A click-through layer-shell overlay for Wayland compositors.
//!
//! The overlay owns a dedicated thread and a `wl_shm` buffer; it never shares
//! an event loop with the presenter. `zwlr_layer_shell_v1` is required. GNOME
//! does not implement it, so `LayerShell::bind` fails there and the caller gets
//! a clear error rather than a silently missing overlay.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
};

use crate::{OverlayBackend, OverlayError, OverlayStats, Scene};

use super::raster;

/// Counters shared between the controller thread and the overlay thread.
#[derive(Debug, Default)]
struct OverlayCounters {
    requested: AtomicU64,
    painted: AtomicU64,
    superseded: AtomicU64,
    errors: AtomicU64,
}

impl OverlayCounters {
    fn stats(&self) -> OverlayStats {
        OverlayStats {
            requested: self.requested.load(Ordering::Relaxed),
            painted: self.painted.load(Ordering::Relaxed),
            superseded: self.superseded.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

/// A layer-shell overlay bound to one output.
pub struct LayerShellOverlay {
    monitor: usize,
    sender: channel::Sender<Message>,
    counters: Arc<OverlayCounters>,
    thread: Option<JoinHandle<()>>,
}

impl LayerShellOverlay {
    /// Spawn the overlay thread on `monitor`.
    ///
    /// Returns once the Wayland connection, layer-shell global and output are
    /// resolved. Surface mapping is asynchronous.
    pub fn spawn(monitor: usize) -> Result<Self, OverlayError> {
        let (sender, receiver) = channel::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let counters = Arc::new(OverlayCounters::default());
        let thread_counters = Arc::clone(&counters);
        let run_counters = Arc::clone(&thread_counters);
        let thread = thread::Builder::new()
            .name("yet-overlay".to_owned())
            .spawn(move || {
                if let Err(error) = run(monitor, receiver, &ready_sender, run_counters) {
                    thread_counters.errors.fetch_add(1, Ordering::Relaxed);
                    let _ = ready_sender.try_send(Err(error));
                }
            })
            .map_err(|error| OverlayError::Unavailable(format!("overlay thread: {error}")))?;

        match ready_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                monitor,
                sender,
                counters,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = sender.send(Message::Shutdown);
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = sender.send(Message::Shutdown);
                let _ = thread.join();
                Err(OverlayError::Unavailable(format!(
                    "overlay did not start: {error}"
                )))
            }
        }
    }

    fn send(&self, message: Message) -> Result<(), OverlayError> {
        self.sender.send(message).map_err(|_| {
            self.counters.errors.fetch_add(1, Ordering::Relaxed);
            OverlayError::Unavailable("overlay thread stopped".to_owned())
        })
    }

    /// A snapshot of the coalescing counters.
    pub fn stats(&self) -> OverlayStats {
        self.counters.stats()
    }
}

impl OverlayBackend for LayerShellOverlay {
    fn show(&mut self, monitor: usize) -> Result<(), OverlayError> {
        if monitor != self.monitor {
            return Err(OverlayError::Unavailable(format!(
                "this overlay is bound to monitor {}, not {monitor}",
                self.monitor
            )));
        }
        self.set_visible(true);
        Ok(())
    }

    fn set_visible(&mut self, visible: bool) {
        let _ = self.sender.send(Message::Visible(visible));
    }

    fn set_passthrough(&mut self, enabled: bool) -> Result<(), OverlayError> {
        self.send(Message::Passthrough(enabled))
    }

    fn present(&mut self, scene: &Scene) -> Result<(), OverlayError> {
        self.counters.requested.fetch_add(1, Ordering::Relaxed);
        self.send(Message::Present(scene.clone()))
    }

    fn stats(&self) -> OverlayStats {
        LayerShellOverlay::stats(self)
    }
}

impl Drop for LayerShellOverlay {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug)]
enum Message {
    Present(Scene),
    Visible(bool),
    Passthrough(bool),
    Shutdown,
}

struct OverlayState {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    qh: QueueHandle<Self>,
    layer: Option<LayerSurface>,
    output: Option<wl_output::WlOutput>,
    empty_region: Region,
    pool: Option<SlotPool>,
    counters: Arc<OverlayCounters>,
    width: u32,
    height: u32,
    scale: i32,
    scene: Scene,
    visible: bool,
    passthrough: bool,
    frame_in_flight: bool,
    pending: bool,
    shutdown: bool,
    error: Option<String>,
}

fn run(
    monitor: usize,
    receiver: channel::Channel<Message>,
    ready: &SyncSender<Result<(), OverlayError>>,
    counters: Arc<OverlayCounters>,
) -> Result<(), OverlayError> {
    let connection = Connection::connect_to_env()
        .map_err(|error| OverlayError::Unavailable(format!("Wayland connection: {error}")))?;
    let (globals, event_queue) = registry_queue_init(&connection)
        .map_err(|error| OverlayError::Unavailable(format!("Wayland registry: {error}")))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| OverlayError::Unavailable(format!("wl_compositor: {error}")))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|error| {
        OverlayError::Unavailable(format!(
            "zwlr_layer_shell_v1 is unavailable ({error}); use KDE, a wlroots/Sway or naked \
             compositor, or run with overlay = \"off\"; GNOME Wayland is not supported"
        ))
    })?;
    let shm = Shm::bind(&globals, &qh)
        .map_err(|error| OverlayError::Unavailable(format!("wl_shm: {error}")))?;
    let empty_region = Region::new(&compositor)
        .map_err(|error| OverlayError::Unavailable(format!("wl_region: {error}")))?;

    let state = OverlayState {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        qh: qh.clone(),
        layer: None,
        output: None,
        empty_region,
        pool: None,
        counters,
        width: 0,
        height: 0,
        scale: 1,
        scene: Scene::new(),
        visible: true,
        passthrough: true,
        frame_in_flight: false,
        pending: false,
        shutdown: false,
        error: None,
    };
    let mut event_loop: EventLoop<OverlayState> = EventLoop::try_new()
        .map_err(|error| OverlayError::Unavailable(format!("event loop: {error}")))?;
    let mut state = state;

    // Populate the output list before selecting the target.
    let mut queue = event_queue;
    queue
        .roundtrip(&mut state)
        .map_err(|error| OverlayError::Unavailable(format!("Wayland roundtrip: {error}")))?;

    let output = state
        .output_state
        .outputs()
        .nth(monitor)
        .ok_or(OverlayError::UnknownMonitor(monitor))?;
    let scale = state
        .output_state
        .info(&output)
        .map(|info| info.scale_factor.max(1))
        .unwrap_or(1);

    let surface = state.compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("yet-rs-overlay"),
        Some(&output),
    );
    layer.set_anchor(Anchor::all());
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    layer.set_input_region(Some(state.empty_region.wl_region()));
    layer.wl_surface().set_buffer_scale(scale);
    layer.commit();

    state.layer = Some(layer);
    state.output = Some(output);
    state.scale = scale;

    let handle = event_loop.handle();
    WaylandSource::new(connection, queue)
        .insert(handle.clone())
        .map_err(|error| OverlayError::Unavailable(format!("Wayland source: {error}")))?;
    handle
        .insert_source(receiver, |event, _, state| match event {
            channel::Event::Msg(message) => state.handle(message),
            channel::Event::Closed => state.shutdown = true,
        })
        .map_err(|error| OverlayError::Unavailable(format!("overlay channel: {error}")))?;

    ready
        .try_send(Ok(()))
        .map_err(|_| OverlayError::Unavailable("overlay caller disappeared".to_owned()))?;

    while !state.shutdown {
        if let Err(error) = event_loop.dispatch(None, &mut state) {
            state.counters.errors.fetch_add(1, Ordering::Relaxed);
            state.error = Some(format!("overlay event loop: {error}"));
            break;
        }
    }
    if let Some(error) = state.error.take() {
        eprintln!("yet-overlay: {error}");
    }
    Ok(())
}

impl OverlayState {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Present(scene) => {
                if self.pending {
                    // The previously queued scene is replaced before it could
                    // be painted; the newest gaze point wins.
                    self.counters.superseded.fetch_add(1, Ordering::Relaxed);
                }
                self.scene = scene;
                if self.frame_in_flight {
                    self.pending = true;
                } else {
                    self.draw();
                }
            }
            Message::Visible(visible) => self.set_visible(visible),
            Message::Passthrough(enabled) => self.set_passthrough(enabled),
            Message::Shutdown => self.shutdown = true,
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(layer) = &self.layer {
            if visible {
                self.draw();
            } else {
                layer.wl_surface().attach(None, 0, 0);
                layer.wl_surface().commit();
            }
        }
    }

    fn set_passthrough(&mut self, enabled: bool) {
        if self.passthrough == enabled {
            return;
        }
        self.passthrough = enabled;
        if let Some(layer) = &self.layer {
            if enabled {
                layer.set_input_region(Some(self.empty_region.wl_region()));
            } else {
                layer.set_input_region(None);
            }
            layer.commit();
        }
    }

    fn draw(&mut self) {
        if !self.visible || self.width == 0 || self.height == 0 {
            return;
        }
        let Some(layer) = &self.layer else {
            return;
        };
        let Some(mut pool) = self.pool.take() else {
            return;
        };

        let surface = layer.wl_surface().clone();
        let qh = self.qh.clone();
        let stride = self.width as usize * 4;
        let result = (|| -> Result<(), OverlayError> {
            let (buffer, canvas) = pool
                .create_buffer(
                    self.width as i32,
                    self.height as i32,
                    stride as i32,
                    wl_shm::Format::Argb8888,
                )
                .map_err(|error| OverlayError::Render(format!("overlay buffer: {error}")))?;
            raster::render(&self.scene, self.width, self.height, stride, canvas)?;
            surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
            surface.frame(&qh, surface.clone());
            buffer
                .attach_to(&surface)
                .map_err(|error| OverlayError::Render(format!("overlay attach: {error}")))?;
            surface.commit();
            Ok(())
        })();

        self.pool = Some(pool);
        match result {
            Ok(()) => {
                self.frame_in_flight = true;
                self.pending = false;
                self.counters.painted.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                self.counters.errors.fetch_add(1, Ordering::Relaxed);
                self.error = Some(error.to_string());
            }
        }
    }
}

impl CompositorHandler for OverlayState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let new_factor = new_factor.max(1);
        if new_factor != self.scale {
            self.scale = new_factor;
            if let Some(layer) = &self.layer {
                layer.wl_surface().set_buffer_scale(new_factor);
            }
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.frame_in_flight = false;
        if self.pending {
            self.draw();
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for OverlayState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.output.as_ref() == Some(&output) {
            if let Some(info) = self.output_state.info(&output) {
                self.scale = info.scale_factor.max(1);
            }
        }
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.output.as_ref() == Some(&output) {
            self.shutdown = true;
        }
    }
}

impl LayerShellHandler for OverlayState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.shutdown = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (logical_width, logical_height) =
            if configure.new_size.0 > 0 && configure.new_size.1 > 0 {
                configure.new_size
            } else {
                self.output
                    .as_ref()
                    .and_then(|output| self.output_state.info(output))
                    .and_then(|info| info.logical_size)
                    .map(|(width, height)| (width.max(1) as u32, height.max(1) as u32))
                    .unwrap_or((1920, 1080))
            };
        let scale = self.scale.max(1) as u32;
        let width = logical_width.saturating_mul(scale);
        let height = logical_height.saturating_mul(scale);
        layer.wl_surface().set_buffer_scale(scale as i32);
        if width == self.width && height == self.height && self.pool.is_some() {
            self.draw();
            return;
        }

        self.width = width;
        self.height = height;
        let pool_size = width as usize * height as usize * 4 * 2;
        match SlotPool::new(pool_size, &self.shm) {
            Ok(pool) => self.pool = Some(pool),
            Err(error) => {
                self.error = Some(format!("overlay pool: {error}"));
                return;
            }
        }
        self.draw();
    }
}

impl ShmHandler for OverlayState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for OverlayState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

delegate_compositor!(OverlayState);
delegate_output!(OverlayState);
delegate_shm!(OverlayState);
delegate_layer!(OverlayState);
delegate_registry!(OverlayState);
