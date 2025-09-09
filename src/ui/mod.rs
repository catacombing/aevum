pub mod create_alarm;
pub mod list_alarms;
pub mod renderer;
pub mod ring_alarm;
pub mod skia;
pub mod window;

use std::time::Instant;

use skia_safe::textlayout::{FontCollection, TextStyle};
use skia_safe::{Canvas, Color4f, FontMgr, Paint, Path, Rect};

use crate::config::{Config, Input};
use crate::geometry::Point;

/// Outer UI padding at scale 1.
pub const OUTSIDE_PADDING: f64 = 10.;

/// Horizontal padding around buttons at scale 1.
pub const BUTTON_PADDING: f64 = 20.;

/// Button height at scale 1.
pub const BUTTON_HEIGHT: f64 = 50.;

/// Stroke width at scale 1.
pub const STROKE_WIDTH: f32 = 2.;

/// Padding around icons inside buttons at scale 1.
const ICON_PADDING: f64 = 10.;

/// Heading text size compared to the normal font size.
const HEADING_SIZE: f32 = 4.;

/// Shared render config cache.
pub struct RenderConfig {
    pub background: Color4f,
    pub font_family: String,
    pub font_size: f64,

    pub fonts: FontCollection,
    pub heading_text_style: TextStyle,
    pub text_style: TextStyle,
    pub input_config: Input,
    pub button_paint: Paint,
    pub icon_paint: Paint,
    pub text_paint: Paint,
}

impl RenderConfig {
    pub fn new(config: &Config) -> Self {
        let mut button_paint = Paint::default();
        button_paint.set_color4f(config.colors.alt_background.as_color4f(), None);

        let mut icon_paint = Paint::default();
        icon_paint.set_color4f(config.colors.foreground.as_color4f(), None);
        icon_paint.set_stroke_width(STROKE_WIDTH);
        icon_paint.set_anti_alias(true);
        icon_paint.set_stroke(true);

        let mut text_paint = Paint::default();
        text_paint.set_color4f(config.colors.foreground.as_color4f(), None);
        text_paint.set_anti_alias(true);

        let font_family = config.font.family.clone();
        let font_size = config.font.size;

        let mut text_style = TextStyle::new();
        text_style.set_foreground_paint(&text_paint);
        text_style.set_font_size(font_size as f32);
        text_style.set_font_families(&[&font_family]);

        let mut heading_text_style = text_style.clone();
        heading_text_style.set_font_size(font_size as f32 * HEADING_SIZE);

        let mut font_collection = FontCollection::new();
        font_collection.set_default_font_manager(FontMgr::new(), None);

        Self {
            fonts: font_collection,
            heading_text_style,
            button_paint,
            font_family,
            text_style,
            icon_paint,
            text_paint,
            font_size,
            background: config.colors.background.as_color4f(),
            input_config: config.input,
        }
    }

    /// Handle config updates.
    ///
    /// Returns `true` when a value was updated and a redraw is required.
    pub fn update_config(&mut self, config: &Config, scale: f64) -> bool {
        let mut dirty = false;

        let alt_background = config.colors.alt_background.as_color4f();
        let foreground = config.colors.foreground.as_color4f();
        let background = config.colors.background.as_color4f();

        if config.font.family != self.font_family {
            self.heading_text_style.set_font_families(&[&config.font.family]);
            self.text_style.set_font_families(&[&config.font.family]);
            self.font_family = config.font.family.clone();
            dirty = true;
        }
        if config.font.size != self.font_size {
            self.heading_text_style.set_font_size((config.font.size * scale) as f32 * HEADING_SIZE);
            self.text_style.set_font_size((config.font.size * scale) as f32);
            self.font_size = config.font.size;
            dirty = true;
        }
        if self.button_paint.color4f() != alt_background {
            self.button_paint.set_color4f(alt_background, None);
            dirty = true;
        }
        if self.text_paint.color4f() != foreground {
            self.text_paint.set_color4f(foreground, None);
            self.icon_paint.set_color4f(foreground, None);
            self.heading_text_style.set_foreground_paint(&self.text_paint);
            self.text_style.set_foreground_paint(&self.text_paint);
            dirty = true;
        }
        if self.background != background {
            self.background = background;
            dirty = true;
        }
        if self.input_config != config.input {
            self.input_config = config.input;
        }

        dirty
    }
}

/// Button icons.
#[derive(Debug)]
enum Icon {
    Confirm,
    Delete,
    Back,
    Plus,
}

impl Icon {
    /// Render the icon inside the specified rectangle.
    fn draw(&self, canvas: &Canvas, scale: f64, paint: &Paint, mut rect: Rect) {
        // Calculate rect and icon dimensions.
        let padding = (ICON_PADDING * scale) as f32;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let size = (width - 2. * padding).min(height - 2. * padding);

        // Center padded square within the original rectangle dimensions;
        rect.left += (width - size) / 2.;
        rect.right -= (width - size) / 2.;
        rect.top += (height - size) / 2.;
        rect.bottom -= (height - size) / 2.;

        match self {
            Icon::Confirm => {
                let mut path = Path::new();
                path.move_to(Point::new(rect.left + size * 0.105, rect.top + size * 0.625));
                path.line_to(Point::new(rect.left + size * 0.438, rect.top + size * 0.875));
                path.line_to(Point::new(rect.left + size * 0.896, rect.top + size * 0.208));
                canvas.draw_path(&path, paint);
            },
            Icon::Delete => {
                let mut path = Path::new();
                path.move_to(Point::new(rect.left + size * 0., rect.top + size * 0.));
                path.line_to(Point::new(rect.left + size * 1., rect.top + size * 1.));
                path.move_to(Point::new(rect.left + size * 1., rect.top + size * 0.));
                path.line_to(Point::new(rect.left + size * 0., rect.top + size * 1.));
                canvas.draw_path(&path, paint);
            },
            Icon::Back => {
                let mut path = Path::new();
                path.move_to(Point::new(rect.left + size * 0.667, rect.top + size * 0.125));
                path.line_to(Point::new(rect.left + size * 0.333, rect.top + size * 0.5));
                path.line_to(Point::new(rect.left + size * 0.667, rect.top + size * 0.875));
                canvas.draw_path(&path, paint);
            },
            Icon::Plus => {
                let mut path = Path::new();
                path.move_to(Point::new(rect.left + size * 0.5, rect.top + size * 0.));
                path.line_to(Point::new(rect.left + size * 0.5, rect.top + size * 1.));
                path.move_to(Point::new(rect.left + size * 0., rect.top + size * 0.5));
                path.line_to(Point::new(rect.left + size * 1., rect.top + size * 0.5));
                canvas.draw_path(&path, paint);
            },
        }
    }
}

/// Scroll velocity state.
#[derive(Default)]
pub struct ScrollVelocity {
    last_tick: Option<Instant>,
    velocity: f64,
}

impl ScrollVelocity {
    /// Check if there is any velocity active.
    pub fn is_moving(&self) -> bool {
        self.velocity != 0.
    }

    /// Set the velocity.
    pub fn set(&mut self, velocity: f64) {
        self.velocity = velocity;
        self.last_tick = None;
    }

    /// Apply and update the current scroll velocity.
    pub fn apply(&mut self, input: &Input, scroll_offset: &mut f64) {
        // No-op without velocity.
        if self.velocity == 0. {
            return;
        }

        // Initialize velocity on the first tick.
        //
        // This avoids applying velocity while the user is still actively scrolling.
        let last_tick = match self.last_tick.take() {
            Some(last_tick) => last_tick,
            None => {
                self.last_tick = Some(Instant::now());
                return;
            },
        };

        // Calculate velocity steps since last tick.
        let now = Instant::now();
        let interval =
            (now - last_tick).as_micros() as f64 / (input.velocity_interval as f64 * 1_000.);

        // Apply and update velocity.
        *scroll_offset += self.velocity * (1. - input.velocity_friction.powf(interval + 1.))
            / (1. - input.velocity_friction);
        self.velocity *= input.velocity_friction.powf(interval);

        // Request next tick if velocity is significant.
        if self.velocity.abs() > 1. {
            self.last_tick = Some(now);
        } else {
            self.velocity = 0.
        }
    }
}
