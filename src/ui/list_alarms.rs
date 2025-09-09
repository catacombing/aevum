//! Alarm overview UI.

use std::mem;

use alarm::Alarms;
use rezz::Alarm;
use skia_safe::textlayout::{ParagraphBuilder, ParagraphStyle};
use skia_safe::{Canvas, Rect};
use time::macros::format_description;
use time::{Duration, OffsetDateTime, UtcOffset};
use tracing::error;

use crate::Config;
use crate::geometry::{Point, Size, rect_contains};
use crate::ui::window::TouchAction as WindowTouchAction;
use crate::ui::{
    BUTTON_HEIGHT, BUTTON_PADDING, Icon, OUTSIDE_PADDING, RenderConfig, ScrollVelocity,
};

/// Horizontal padding around the alarms list at scale 1.
const ALARMS_PADDING: f64 = 25.;

/// Height of a single alarm in the list at scale 1.
const ALARM_HEIGHT: f64 = 80.;

/// Width and height of the alarm deletion button at scale 1.
const DELETE_SIZE: f64 = 40.;

/// Alarm list UI state.
pub struct ListAlarms {
    velocity: ScrollVelocity,
    touch_state: TouchState,
    scroll_offset: f64,

    size: Size<f32>,
    scale: f64,

    alarms: Vec<Alarm>,

    dirty: bool,
}

impl Default for ListAlarms {
    fn default() -> Self {
        Self {
            dirty: true,
            scale: 1.,
            scroll_offset: Default::default(),
            touch_state: Default::default(),
            velocity: Default::default(),
            alarms: Default::default(),
            size: Default::default(),
        }
    }
}

impl ListAlarms {
    /// Render current UI state.
    pub fn draw(&mut self, size: Size, scale: f64, canvas: &Canvas, render_config: &RenderConfig) {
        self.dirty = false;

        self.size = size.into();
        self.scale = scale;

        // Animate scroll velocity.
        self.velocity.apply(&render_config.input_config, &mut self.scroll_offset);

        // Ensure offset is correct in case alarms were deleted or geometry changed.
        self.clamp_scroll_offset();

        // Clear background.
        canvas.clear(render_config.background);

        // Define clipping mask for alarms.
        let mut alarm_rect = Self::last_alarm_rect(self.size, scale);
        let alarm_clip_rect = Rect { top: 0., left: 0., ..alarm_rect };
        canvas.save();
        canvas.clip_rect(alarm_clip_rect, None, Some(false));

        // Draw alarms list.
        let alarm_height = alarm_rect.bottom - alarm_rect.top;
        let alarms_end = alarm_rect.bottom;
        alarm_rect.top += self.scroll_offset as f32;
        alarm_rect.bottom += self.scroll_offset as f32;
        for alarm in self.alarms.iter().rev() {
            if alarm_rect.bottom > 0. && alarm_rect.top < alarms_end {
                self.draw_alarm(canvas, render_config, alarm_rect, alarm);
            }

            // Advance position to the next alarm location.
            alarm_rect.top -= alarm_height;
            alarm_rect.bottom -= alarm_height;
        }

        // Reset alarms clipping mask.
        canvas.restore();

        // Draw the new alarm button.
        let new_rect = Self::new_button_rect(self.size, scale);
        canvas.draw_rect(new_rect, &render_config.button_paint);
        Icon::Plus.draw(canvas, scale, &render_config.icon_paint, new_rect);
    }

    /// Draw a single alarm.
    fn draw_alarm(&self, canvas: &Canvas, render_config: &RenderConfig, rect: Rect, alarm: &Alarm) {
        // Draw the delete icon.
        let mut delete_rect = Self::delete_alarm_rect(self.size, self.scale);
        delete_rect.left += rect.left;
        delete_rect.top += rect.top;
        delete_rect.right += rect.left;
        delete_rect.bottom += rect.top;
        Icon::Delete.draw(canvas, self.scale, &render_config.icon_paint, delete_rect);

        // Convert alarm's unix time to local time in HH:MM and YYYY-mm-dd format.
        let utc_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        let time = OffsetDateTime::UNIX_EPOCH + Duration::seconds(alarm.unix_time);
        let local_time = time.to_offset(utc_offset);
        let time_format = format_description!("[hour]:[minute]");
        let time_str = local_time.format(&time_format).unwrap();
        let date_format = format_description!("[year]-[month]-[day]");
        let date_str = local_time.format(&date_format).unwrap();

        // Create time label paragraph.

        // Setup text style.
        let mut time_style = ParagraphStyle::new();
        time_style.set_text_style(&render_config.heading_text_style);

        // Create and layout the paragraph.
        let mut time_builder = ParagraphBuilder::new(&time_style, &render_config.fonts);
        time_builder.add_text(time_str);
        let mut time_paragraph = time_builder.build();
        time_paragraph.layout(rect.right - rect.left);

        // Create date label paragraph.

        // Setup text style.
        let mut date_style = ParagraphStyle::new();
        date_style.set_text_style(&render_config.text_style);

        // Create and layout the paragraph.
        let mut date_builder = ParagraphBuilder::new(&date_style, &render_config.fonts);
        date_builder.add_text(date_str);
        let mut date_paragraph = date_builder.build();
        date_paragraph.layout(rect.right - rect.left);

        //

        // Calculate vertical text position.
        let time_height = time_paragraph.height();
        let y_offset = (rect.bottom - rect.top - time_height - date_paragraph.height()) / 2.;
        let time_y = rect.top + y_offset;

        // Draw both labels once layout is calculated.
        time_paragraph.paint(canvas, Point::new(rect.left, time_y));
        date_paragraph.paint(canvas, Point::new(rect.left, time_y + time_height));
    }

    /// Check whether the UI requires a redraw.
    pub fn dirty(&self) -> bool {
        self.dirty || self.velocity.is_moving()
    }

    /// Update the list of alarms.
    pub fn set_alarms(&mut self, mut alarms: Vec<Alarm>) {
        self.alarms.clear();
        self.alarms.append(&mut alarms);

        self.dirty = true;
    }

    /// Handle touch press.
    pub fn touch_down(&mut self, logical_point: Point<f64>) {
        // Cancel velocity when a new touch sequence starts.
        self.velocity.set(0.);

        // Convert position to physical space.
        let point = logical_point * self.scale;
        self.touch_state.point = point;
        self.touch_state.start = point;

        // Get button geometries.
        let new_rect = Self::new_button_rect(self.size, self.scale);

        if rect_contains(new_rect, point) {
            self.touch_state.action = TouchAction::CreateAlarm;
        } else if let Some((alarm, delete)) = self.alarm_at(point.into()) {
            self.touch_state.action = TouchAction::AlarmTap(alarm.id.clone(), delete);
        } else {
            self.touch_state.action = TouchAction::None;
        }
    }

    /// Handle touch motion.
    pub fn touch_motion(&mut self, config: &Config, logical_point: Point<f64>) {
        // Update touch position.
        let point = logical_point * self.scale;
        let old_point = mem::replace(&mut self.touch_state.point, point);

        // Handle alarm list scrolling.
        if let TouchAction::AlarmTap(..) | TouchAction::AlarmDrag = self.touch_state.action {
            // Ignore dragging until tap distance limit is exceeded.
            let max_tap_distance = config.input.max_tap_distance;
            let delta = self.touch_state.point - self.touch_state.start;
            if delta.x.powi(2) + delta.y.powi(2) <= max_tap_distance {
                return;
            }
            self.touch_state.action = TouchAction::AlarmDrag;

            // Calculate current scroll velocity.
            let delta = self.touch_state.point.y - old_point.y;
            self.velocity.set(delta);

            // Immediately start moving the tabs list.
            let old_offset = self.scroll_offset;
            self.scroll_offset += delta;
            self.clamp_scroll_offset();
            self.dirty |= self.scroll_offset != old_offset;
        }
    }

    /// Handle touch release.
    pub fn touch_up(&mut self) -> WindowTouchAction {
        match mem::take(&mut self.touch_state.action) {
            // Switch to the alarm view.
            TouchAction::CreateAlarm => {
                let rect = Self::new_button_rect(self.size, self.scale);
                if rect_contains(rect, self.touch_state.point) {
                    return WindowTouchAction::CreateAlarmView;
                }
            },
            // Remove an alarm.
            TouchAction::AlarmTap(id, true) => {
                tokio::spawn(async move {
                    if let Err(err) = Alarms.remove(id).await {
                        error!("Failed to remove alarm: {err}");
                    }
                });
            },
            _ => (),
        }

        WindowTouchAction::None
    }

    /// Physical rectangle of the new alarm button.
    fn new_button_rect(size: Size<f32>, scale: f64) -> Rect {
        let padding = (OUTSIDE_PADDING * scale) as f32;

        let button_width = size.width - 2. * padding;
        let button_height = (BUTTON_HEIGHT * scale) as f32;

        let y = size.height - button_height - padding;
        let x = (size.width - button_width) / 2.;

        Rect::new(x, y, x + button_width, y + button_height)
    }

    /// Physical rectangle of the bottommost alarm.
    fn last_alarm_rect(size: Size<f32>, scale: f64) -> Rect {
        let new_rect = Self::new_button_rect(size, scale);
        let outside_padding = (OUTSIDE_PADDING * scale) as f32;
        let alarms_padding = (ALARMS_PADDING * scale) as f32;
        let button_padding = (BUTTON_PADDING * scale) as f32;

        let width = size.width - 2. * outside_padding - 2. * alarms_padding;
        let height = (ALARM_HEIGHT * scale) as f32;

        let y = new_rect.top - button_padding - height;
        let x = outside_padding + alarms_padding;

        Rect::new(x, y, x + width, y + height)
    }

    /// Physical rectangle of the alarm removal button relative to the alarm
    /// origin.
    fn delete_alarm_rect(size: Size<f32>, scale: f64) -> Rect {
        let alarm_rect = Self::last_alarm_rect(size, scale);

        let size = (DELETE_SIZE * scale) as f32;
        let padding = (alarm_rect.bottom - alarm_rect.top - size) / 2.;

        let x = alarm_rect.right - alarm_rect.left - padding - size;
        let y = padding;

        Rect::new(x, y, x + size, y + size)
    }

    /// Get alarm at the specified location.
    fn alarm_at(&self, mut point: Point<f32>) -> Option<(&Alarm, bool)> {
        let last_alarm_rect = Self::last_alarm_rect(self.size, self.scale);

        // Short-circuit if point is outside the alarm list.
        if point.x < last_alarm_rect.left
            || point.x >= last_alarm_rect.right
            || point.y >= last_alarm_rect.bottom
        {
            return None;
        }

        // Apply current scroll offset.
        point.y -= self.scroll_offset as f32;

        // Find entry index at the specified offset.
        let alarm_height = last_alarm_rect.bottom - last_alarm_rect.top;
        let bottom_relative = last_alarm_rect.bottom - point.y;
        let rindex = (bottom_relative / alarm_height).floor() as usize;
        let index = self.alarms.len().saturating_sub(rindex + 1);

        // Check if touch is within close button bounds.
        let delete_alarm_rect = Self::delete_alarm_rect(self.size, self.scale);
        let relative_x = (point.x - last_alarm_rect.left) as f64;
        let relative_y = (alarm_height - 1. - (bottom_relative % alarm_height)) as f64;
        let delete = rect_contains(delete_alarm_rect, Point::new(relative_x, relative_y));

        Some((&self.alarms[index], delete))
    }

    /// Clamp alarm list viewport offset.
    fn clamp_scroll_offset(&mut self) {
        let old_offset = self.scroll_offset;
        let max_offset = self.max_scroll_offset() as f64;
        self.scroll_offset = self.scroll_offset.clamp(0., max_offset);

        // Cancel velocity after reaching the scroll limit.
        if old_offset != self.scroll_offset {
            self.velocity.set(0.);
            self.dirty = true;
        }
    }

    /// Get maximum alarm list viewport offset.
    fn max_scroll_offset(&self) -> usize {
        let last_alarm_rect = Self::last_alarm_rect(self.size, self.scale);
        let button_padding = (BUTTON_PADDING * self.scale) as f32;

        let alarm_height = last_alarm_rect.bottom - last_alarm_rect.top;
        let total_height = alarm_height * self.alarms.len() as f32;
        let max_offset = total_height - last_alarm_rect.bottom + button_padding;

        max_offset.ceil().max(0.) as usize
    }
}

/// Touch event tracking.
#[derive(Default)]
struct TouchState {
    action: TouchAction,
    start: Point<f64>,
    point: Point<f64>,
}

/// Intention of a touch sequence.
#[derive(Default)]
enum TouchAction {
    #[default]
    None,
    CreateAlarm,
    AlarmTap(String, bool),
    AlarmDrag,
}
