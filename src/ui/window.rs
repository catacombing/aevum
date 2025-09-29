//! Wayland window rendering.

use std::mem;
use std::ptr::NonNull;

use alarm::Alarms;
use alarm::audio::AlarmSound;
use glutin::display::{Display, DisplayApiPreference};
use raw_window_handle::{RawDisplayHandle, WaylandDisplayHandle};
use rezz::Alarm;
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::{Window as XdgWindow, WindowDecorations};
use tracing::error;

use crate::config::Config;
use crate::geometry::{Point, Size};
use crate::ui::create_alarm::CreateAlarm;
use crate::ui::list_alarms::ListAlarms;
use crate::ui::renderer::Renderer;
use crate::ui::ring_alarm::RingAlarm;
use crate::ui::skia::Canvas;
use crate::ui::{RenderConfig, STROKE_WIDTH};
use crate::wayland::ProtocolStates;
use crate::{Error, State};

/// Wayland window.
pub struct Window {
    pub queue: QueueHandle<State>,
    pub initial_draw_done: bool,

    connection: Connection,
    xdg_window: XdgWindow,
    viewport: WpViewport,
    renderer: Renderer,

    create_alarm: CreateAlarm,
    list_alarms: ListAlarms,
    ring_alarm: RingAlarm,
    view: View,

    render_config: RenderConfig,
    canvas: Canvas,

    stalled: bool,
    dirty: bool,
    size: Size,
    scale: f64,
}

impl Window {
    pub fn new(
        protocol_states: &ProtocolStates,
        connection: Connection,
        queue: QueueHandle<State>,
        config: &Config,
    ) -> Result<Self, Error> {
        // Get EGL display.
        let display = NonNull::new(connection.backend().display_ptr().cast()).unwrap();
        let wayland_display = WaylandDisplayHandle::new(display);
        let raw_display = RawDisplayHandle::Wayland(wayland_display);
        let egl_display = unsafe { Display::new(raw_display, DisplayApiPreference::Egl)? };

        // Create the XDG shell window.
        let surface = protocol_states.compositor.create_surface(&queue);
        let xdg_window = protocol_states.xdg_shell.create_window(
            surface.clone(),
            WindowDecorations::RequestClient,
            &queue,
        );
        xdg_window.set_title("Aevum");
        xdg_window.set_app_id("Aevum");
        xdg_window.commit();

        // Create surface's Wayland global handles.
        if let Some(fractional_scale) = &protocol_states.fractional_scale {
            fractional_scale.fractional_scaling(&queue, &surface);
        }
        let viewport = protocol_states.viewporter.viewport(&queue, &surface);

        // Create OpenGL renderer.
        let renderer = Renderer::new(egl_display, surface);

        // Default to a reasonable default size.
        let size = Size { width: 360, height: 720 };

        Ok(Self {
            connection,
            xdg_window,
            viewport,
            renderer,
            queue,
            size,
            render_config: RenderConfig::new(config),
            stalled: true,
            dirty: true,
            scale: 1.,
            initial_draw_done: Default::default(),
            create_alarm: Default::default(),
            list_alarms: Default::default(),
            ring_alarm: Default::default(),
            canvas: Default::default(),
            view: Default::default(),
        })
    }

    /// Redraw the window.
    pub fn draw(&mut self) {
        // Stall rendering if nothing changed since last redraw.
        if !self.dirty() {
            self.stalled = true;
            return;
        }
        self.initial_draw_done = true;
        self.dirty = false;

        // Update viewporter logical render size.
        //
        // NOTE: This must be done every time we draw with Sway; it is not
        // persisted when drawing with the same surface multiple times.
        self.viewport.set_destination(self.size.width as i32, self.size.height as i32);

        // Mark entire window as damaged.
        let wl_surface = self.xdg_window.wl_surface();
        wl_surface.damage(0, 0, self.size.width as i32, self.size.height as i32);

        // Render the window content.
        let size = self.size * self.scale;
        self.renderer.draw(size, |renderer| {
            let config = &self.render_config;
            self.canvas.draw(renderer.skia_config(), size, |canvas| match &self.view {
                View::ListAlarms => self.list_alarms.draw(size, self.scale, canvas, config),
                View::CreateAlarm => self.create_alarm.draw(size, self.scale, canvas, config),
                View::RingAlarm(alarm, _) => {
                    self.ring_alarm.draw(size, self.scale, canvas, config, alarm);
                },
            });
        });

        // Request a new frame.
        wl_surface.frame(&self.queue, wl_surface.clone());

        // Apply surface changes.
        wl_surface.commit();
    }

    /// Unstall the renderer.
    ///
    /// This will render a new frame if there currently is no frame request
    /// pending.
    pub fn unstall(&mut self) {
        // Ignore if unstalled or request came from background engine.
        if !mem::take(&mut self.stalled) {
            return;
        }

        // Redraw immediately to unstall rendering.
        self.draw();
        let _ = self.connection.flush();
    }

    /// Update the list of alarms.
    pub fn set_alarms(&mut self, alarms: Vec<Alarm>) {
        self.list_alarms.set_alarms(alarms);

        self.unstall();
    }

    /// Start alarm audio playback.
    pub fn ring(&mut self, mut alarm: Alarm) {
        // Immediately remove the alarm, to avoid other clients picking it up.
        let id = mem::take(&mut alarm.id);
        tokio::spawn(async move {
            if let Err(err) = Alarms.remove(id).await {
                error!("Failed to remove active alarm: {err}");
            }
        });

        // Start alarm sound playback.
        let sound = match AlarmSound::play() {
            Ok(sound) => sound,
            Err(err) => {
                error!("Failed to play alarm: {err}");
                return;
            },
        };

        self.view = View::RingAlarm(alarm, sound);
        self.dirty = true;

        self.unstall();
    }

    /// Update the window's logical size.
    pub fn set_size(&mut self, compositor: &CompositorState, size: Size) {
        if self.size == size {
            return;
        }

        self.size = size;
        self.dirty = true;

        // Update the window's opaque region.
        //
        // This is done here since it can only change on resize, but the commit happens
        // atomically on redraw.
        if let Ok(region) = Region::new(compositor) {
            region.add(0, 0, size.width as i32, size.height as i32);
            self.xdg_window.wl_surface().set_opaque_region(Some(region.wl_region()));
        }

        self.unstall();
    }

    /// Update the window's DPI factor.
    pub fn set_scale_factor(&mut self, scale: f64) {
        if self.scale == scale {
            return;
        }

        self.scale = scale;
        self.dirty = true;

        // Update scale-based render config cache data.
        self.render_config.text_style.set_font_size((self.render_config.font_size * scale) as f32);
        self.render_config.icon_paint.set_stroke_width(STROKE_WIDTH * scale as f32);

        self.unstall();
    }

    /// Handle config updates.
    pub fn update_config(&mut self, config: &Config) {
        if self.render_config.update_config(config, self.scale) {
            self.dirty = true;
            self.unstall();
        }
    }

    /// Handle touch press.
    pub fn touch_down(&mut self, point: Point<f64>) {
        match self.view {
            View::ListAlarms => self.list_alarms.touch_down(point),
            View::CreateAlarm => self.create_alarm.touch_down(point),
            View::RingAlarm(..) => self.ring_alarm.touch_down(point),
        }
        self.unstall();
    }

    /// Handle touch motion.
    pub fn touch_motion(&mut self, config: &Config, point: Point<f64>) {
        match self.view {
            View::ListAlarms => self.list_alarms.touch_motion(config, point),
            View::CreateAlarm => self.create_alarm.touch_motion(point),
            View::RingAlarm(..) => self.ring_alarm.touch_motion(point),
        }
        self.unstall();
    }

    /// Handle touch release.
    pub fn touch_up(&mut self) {
        let action = match &self.view {
            View::ListAlarms => self.list_alarms.touch_up(),
            View::CreateAlarm => self.create_alarm.touch_up(&self.render_config.input_config),
            View::RingAlarm(..) => self.ring_alarm.touch_up(),
        };

        // Execute requested window actions.
        match action {
            TouchAction::None => (),
            TouchAction::ListAlarmsView => {
                self.view = View::ListAlarms;
                self.dirty = true;
            },
            TouchAction::CreateAlarmView => {
                self.view = View::CreateAlarm;
                self.create_alarm.reset();
                self.dirty = true;
            },
        }

        self.unstall();
    }

    /// Check whether the UI requires a redraw.
    fn dirty(&self) -> bool {
        if self.dirty {
            return true;
        }

        match self.view {
            View::ListAlarms => self.list_alarms.dirty(),
            View::CreateAlarm => self.create_alarm.dirty(),
            View::RingAlarm(..) => self.ring_alarm.dirty(),
        }
    }
}

/// Available UI views.
#[derive(Default)]
enum View {
    #[default]
    ListAlarms,
    CreateAlarm,
    RingAlarm(Alarm, #[allow(unused)] AlarmSound),
}

/// Window touch actions triggerable by downstream UIs.
pub enum TouchAction {
    None,
    ListAlarmsView,
    CreateAlarmView,
}
