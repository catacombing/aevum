//! Active ringing alarm UI.

use rezz::Alarm;
use skia_safe::textlayout::{ParagraphBuilder, ParagraphStyle, TextAlign};
use skia_safe::{Canvas, Rect};
use time::macros::format_description;
use time::{Duration, OffsetDateTime, UtcOffset};

use crate::geometry::{Point, Size, rect_contains};
use crate::ui::window::TouchAction as WindowTouchAction;
use crate::ui::{BUTTON_HEIGHT, OUTSIDE_PADDING, RenderConfig};

/// Active ringing alarm UI state.
pub struct RingAlarm {
    touch_state: TouchState,

    size: Size<f32>,
    scale: f64,

    dirty: bool,
}

impl Default for RingAlarm {
    fn default() -> Self {
        Self { dirty: true, scale: 1., touch_state: Default::default(), size: Default::default() }
    }
}

impl RingAlarm {
    /// Render current UI state.
    pub fn draw(
        &mut self,
        size: Size,
        scale: f64,
        canvas: &Canvas,
        render_config: &RenderConfig,
        alarm: &Alarm,
    ) {
        self.dirty = false;

        self.size = size.into();
        self.scale = scale;

        // Clear background.
        canvas.clear(render_config.background);

        // Draw alarm details.

        // Convert Alarm's unix timestamp to a local time.
        let utc_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        let time = OffsetDateTime::UNIX_EPOCH + Duration::seconds(alarm.unix_time);
        let local_time = time.to_offset(utc_offset);
        let time_format = format_description!("[hour]:[minute]");
        let time_str = local_time.format(&time_format).unwrap();

        // Configure text rendering style.
        let mut time_style = ParagraphStyle::new();
        time_style.set_text_style(&render_config.heading_text_style);
        time_style.set_text_align(TextAlign::Center);

        // Perform text shaping and layout.
        let time_rect = Self::time_text_rect(self.size);
        let mut time_builder = ParagraphBuilder::new(&time_style, &render_config.fonts);
        time_builder.add_text(time_str);
        let mut time_paragraph = time_builder.build();
        time_paragraph.layout(time_rect.right - time_rect.left);

        // Draw label in the center of the button.
        let y_offset = (time_rect.bottom - time_rect.top - time_paragraph.height()) / 2.;
        let point = Point::new(time_rect.left, time_rect.top + y_offset);
        time_paragraph.paint(canvas, point);

        // Draw stop button.

        // Draw button background.
        let stop_rect = Self::stop_button_rect(self.size, self.scale);
        canvas.draw_rect(stop_rect, &render_config.button_paint);

        // Configure text rendering style.
        let mut stop_style = ParagraphStyle::new();
        stop_style.set_text_style(&render_config.text_style);
        stop_style.set_text_align(TextAlign::Center);

        // Perform text shaping and layout.
        let mut stop_builder = ParagraphBuilder::new(&stop_style, &render_config.fonts);
        stop_builder.add_text("Stop Alarm");
        let mut stop_paragraph = stop_builder.build();
        stop_paragraph.layout(stop_rect.right - stop_rect.left);

        // Draw label in the center of the button.
        let y_offset = (stop_rect.bottom - stop_rect.top - stop_paragraph.height()) / 2.;
        let point = Point::new(stop_rect.left, stop_rect.top + y_offset);
        stop_paragraph.paint(canvas, point);
    }

    /// Check whether the UI requires a redraw.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Handle touch press.
    pub fn touch_down(&mut self, logical_point: Point<f64>) {
        // Convert position to physical space.
        let point = logical_point * self.scale;
        self.touch_state.point = point;

        // Get button geometries.
        let stop_rect = Self::stop_button_rect(self.size, self.scale);

        if rect_contains(stop_rect, point) {
            self.touch_state.action = TouchAction::Stop;
        } else {
            self.touch_state.action = TouchAction::None;
        }
    }

    /// Handle touch motion.
    pub fn touch_motion(&mut self, logical_point: Point<f64>) {
        // Update touch position.
        let point = logical_point * self.scale;
        self.touch_state.point = point;
    }

    /// Handle touch release.
    pub fn touch_up(&mut self) -> WindowTouchAction {
        // Return to lists view, thereby automatically cancelling the alarm playback.
        if let TouchAction::Stop = self.touch_state.action {
            let rect = Self::stop_button_rect(self.size, self.scale);
            if rect_contains(rect, self.touch_state.point) {
                return WindowTouchAction::ListAlarmsView;
            }
        }

        WindowTouchAction::None
    }

    /// Physical rectangle of the ringing alarm's time label.
    fn time_text_rect(size: Size<f32>) -> Rect {
        Rect::new(0., 0., size.width, size.height)
    }

    /// Physical rectangle of the stop ringing button.
    fn stop_button_rect(size: Size<f32>, scale: f64) -> Rect {
        let padding = (OUTSIDE_PADDING * scale) as f32;

        let button_width = size.width - 2. * padding;
        let button_height = (BUTTON_HEIGHT * scale) as f32;

        let y = size.height - button_height - padding;
        let x = (size.width - button_width) / 2.;

        Rect::new(x, y, x + button_width, y + button_height)
    }
}

/// Touch event tracking.
#[derive(Default)]
struct TouchState {
    action: TouchAction,
    point: Point<f64>,
}

/// Intention of a touch sequence.
#[derive(Default)]
enum TouchAction {
    #[default]
    None,
    Stop,
}
