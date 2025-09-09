//! Alarm creation UI.

use std::mem;

use alarm::Alarms;
use rezz::Alarm;
use skia_safe::textlayout::{ParagraphBuilder, ParagraphStyle, TextAlign};
use skia_safe::{Canvas, Rect};
use time::{Duration, OffsetDateTime, Time};
use tracing::error;
use uuid::Uuid;

use crate::geometry::{Point, Size, rect_contains};
use crate::ui::window::TouchAction as WindowTouchAction;
use crate::ui::{
    BUTTON_HEIGHT, BUTTON_PADDING, Icon, OUTSIDE_PADDING, RenderConfig, ScrollVelocity,
};

/// Width and height of time wheel items at scale 1.
const CAROUSEL_ITEM_SIZE: f64 = 75.;

/// Space between carousel wheels at scale 1.
const CAROUSEL_SPACE: f64 = 50.;

/// Alarm ring duration in seconds.
const RING_DURATION: u32 = 15 * 60;

/// Alarm creation UI state.
pub struct CreateAlarm {
    touch_state: TouchState,

    minute_carousel: TextCarousel,
    hour_carousel: TextCarousel,

    size: Size<f32>,
    scale: f64,

    dirty: bool,
}

impl Default for CreateAlarm {
    fn default() -> Self {
        let hours = (0..24).map(|hour| format!("{hour:0>2}")).collect();
        let hour_carousel = TextCarousel::new(hours);
        let minutes = (0..60).step_by(5).map(|minute| format!("{minute:0>2}")).collect();
        let minute_carousel = TextCarousel::new(minutes);

        Self {
            minute_carousel,
            hour_carousel,
            dirty: true,
            scale: 1.,
            touch_state: Default::default(),
            size: Default::default(),
        }
    }
}

impl CreateAlarm {
    /// Render current UI state.
    pub fn draw(&mut self, size: Size, scale: f64, canvas: &Canvas, render_config: &RenderConfig) {
        self.dirty = false;

        self.size = size.into();
        self.scale = scale;

        // Clear background.
        canvas.clear(render_config.background);

        // Draw time selection wheels.
        let hour_rect = Self::hour_carousel_rect(self.size, scale);
        self.hour_carousel.draw(scale, canvas, render_config, hour_rect);
        let minute_rect = Self::minute_carousel_rect(self.size, scale);
        self.minute_carousel.draw(scale, canvas, render_config, minute_rect);

        self.draw_delta_text(canvas, render_config);

        // Draw the cancel creation button.
        let back_rect = Self::back_button_rect(self.size, scale);
        canvas.draw_rect(back_rect, &render_config.button_paint);
        Icon::Back.draw(canvas, scale, &render_config.icon_paint, back_rect);

        // Draw the confirm creation button.
        let confirm_rect = Self::confirm_button_rect(self.size, scale);
        canvas.draw_rect(confirm_rect, &render_config.button_paint);
        Icon::Confirm.draw(canvas, scale, &render_config.icon_paint, confirm_rect);
    }

    /// Draw text showing delta between current and alarm time.
    fn draw_delta_text(&self, canvas: &Canvas, render_config: &RenderConfig) {
        let delta_rect = Self::delta_text_rect(self.size, self.scale);

        // Setup text style for alarm delta.
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_style(&render_config.text_style);
        paragraph_style.set_text_align(TextAlign::Center);

        // Create and layout the paragraph.
        let mut paragraph_builder = ParagraphBuilder::new(&paragraph_style, &render_config.fonts);
        paragraph_builder.add_text(self.delta_text());
        let mut paragraph = paragraph_builder.build();
        paragraph.layout(delta_rect.right - delta_rect.left);

        // Center paragraph vertically inside its rect.
        let delta_y_offset = (delta_rect.bottom - delta_rect.top - paragraph.height()) / 2.;
        paragraph.paint(canvas, Point::new(delta_rect.left, delta_rect.top + delta_y_offset));
    }

    /// Check whether the UI requires a redraw.
    pub fn dirty(&self) -> bool {
        self.dirty || self.hour_carousel.dirty() || self.minute_carousel.dirty()
    }

    /// Reset the time selection wheels to the time five minutes from now.
    pub fn reset(&mut self) {
        // Get current time.
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let mut time = now.time();

        // Add five minutes to ensure time is in the future.
        time += Duration::minutes(5);

        // Scroll to the time one minute from now.
        self.minute_carousel.scroll_to(time.minute() as usize / 5);
        self.hour_carousel.scroll_to(time.hour() as usize);
    }

    /// Handle touch press.
    pub fn touch_down(&mut self, logical_point: Point<f64>) {
        // Convert position to physical space.
        let point = logical_point * self.scale;
        self.touch_state.point = point;
        self.touch_state.start = point;

        // Get button geometries.
        let confirm_rect = Self::confirm_button_rect(self.size, self.scale);
        let minute_rect = Self::minute_carousel_rect(self.size, self.scale);
        let hour_rect = Self::hour_carousel_rect(self.size, self.scale);
        let back_rect = Self::back_button_rect(self.size, self.scale);

        if rect_contains(confirm_rect, point) {
            self.touch_state.action = TouchAction::Confirm;
        } else if rect_contains(back_rect, point) {
            self.touch_state.action = TouchAction::Back;
        } else if rect_contains(minute_rect, point) {
            self.touch_state.action = TouchAction::MinuteCarousel;

            self.minute_carousel.touch_down(point);
        } else if rect_contains(hour_rect, point) {
            self.touch_state.action = TouchAction::HourCarousel;

            self.hour_carousel.touch_down(point);
        } else {
            self.touch_state.action = TouchAction::None;
        }
    }

    /// Handle touch motion.
    pub fn touch_motion(&mut self, logical_point: Point<f64>) {
        // Update touch position.
        let point = logical_point * self.scale;
        self.touch_state.point = point;

        match self.touch_state.action {
            TouchAction::MinuteCarousel => self.minute_carousel.touch_motion(point),
            TouchAction::HourCarousel => self.hour_carousel.touch_motion(point),
            _ => (),
        }
    }

    /// Handle touch release.
    pub fn touch_up(&mut self) -> WindowTouchAction {
        match self.touch_state.action {
            // Switch to the list view.
            TouchAction::Back => {
                let rect = Self::back_button_rect(self.size, self.scale);
                if rect_contains(rect, self.touch_state.point) {
                    return WindowTouchAction::ListAlarmsView;
                }
            },
            // Create a new alarm.
            TouchAction::Confirm => {
                let rect = Self::confirm_button_rect(self.size, self.scale);
                if rect_contains(rect, self.touch_state.point) {
                    // Get alarm time as unix timestamp.
                    let alarm_time = self.alarm_time();
                    let unix_time = (alarm_time - OffsetDateTime::UNIX_EPOCH).whole_seconds();

                    // Stage new alarm.
                    let id = Uuid::new_v4().to_string();
                    let alarm = Alarm::new(&id, unix_time, RING_DURATION);
                    tokio::spawn(async {
                        if let Err(err) = Alarms.add(alarm).await {
                            error!("Failed to create alarm: {err}");
                        }
                    });

                    // Return to the list view.
                    return WindowTouchAction::ListAlarmsView;
                }
            },
            _ => (),
        }

        WindowTouchAction::None
    }

    /// Physical rectangle of the cancel button.
    fn back_button_rect(size: Size<f32>, scale: f64) -> Rect {
        let button_size = (BUTTON_HEIGHT * scale) as f32;
        let padding = (OUTSIDE_PADDING * scale) as f32;

        let y = size.height - button_size - padding;
        let x = padding;

        Rect::new(x, y, x + button_size, y + button_size)
    }

    /// Physical rectangle of the confirm button.
    fn confirm_button_rect(size: Size<f32>, scale: f64) -> Rect {
        let button_size = (BUTTON_HEIGHT * scale) as f32;
        let padding = (OUTSIDE_PADDING * scale) as f32;

        let y = size.height - button_size - padding;
        let x = size.width - button_size - padding;

        Rect::new(x, y, x + button_size, y + button_size)
    }

    /// Physical rectangle of the time delta text.
    fn delta_text_rect(size: Size<f32>, scale: f64) -> Rect {
        let back_rect = Self::back_button_rect(size, scale);
        let back_padding = (BUTTON_PADDING * scale) as f32;

        let height = back_rect.bottom - back_rect.top;
        let y = back_rect.top - back_padding - height;

        Rect::new(0., y, size.width, y + height)
    }

    /// Physical rectangle of the hour selection wheel.
    fn hour_carousel_rect(size: Size<f32>, scale: f64) -> Rect {
        let delta_rect = Self::delta_text_rect(size, scale);
        let delta_padding = (BUTTON_PADDING * scale) as f32;
        let item_size = (CAROUSEL_ITEM_SIZE * scale) as f32;
        let space = (CAROUSEL_SPACE * scale) as f32;

        let height = item_size * 3.;
        let y = delta_rect.top - delta_padding - height;
        let x = size.width / 2. - item_size - space / 2.;

        Rect::new(x, y, x + item_size, y + height)
    }

    /// Physical rectangle of the minute selection wheel.
    fn minute_carousel_rect(size: Size<f32>, scale: f64) -> Rect {
        let hour_rect = Self::hour_carousel_rect(size, scale);
        let item_size = (CAROUSEL_ITEM_SIZE * scale) as f32;
        let space = (CAROUSEL_SPACE * scale) as f32;

        let x = size.width / 2. + space / 2.;

        Rect::new(x, hour_rect.top, x + item_size, hour_rect.bottom)
    }

    /// Text label for delta between current and alarm time.
    fn delta_text(&self) -> String {
        // Get current and alarm time.
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let alarm_time = self.alarm_time();

        // Get hours/minutes until alarm.
        let delta = alarm_time - now;
        let hours = delta.whole_hours();
        let minutes = delta.whole_minutes() - 60 * hours;

        // Format hours/minutes.
        let minute_unit = if minutes > 1 { "minutes" } else { "minute" };
        if hours == 0 && minutes == 0 {
            String::from("now")
        } else if hours == 0 {
            format!("in {minutes} {minute_unit}")
        } else {
            let hour_unit = if hours > 1 { "hours" } else { "hour" };
            format!("in {hours} {hour_unit} and {minutes} {minute_unit}")
        }
    }

    /// Get the currently selected alarm time.
    fn alarm_time(&self) -> OffsetDateTime {
        let minute = self.minute_carousel.value();
        let hour = self.hour_carousel.value();

        let time = Time::from_hms(hour, minute, 0).unwrap();

        // Get next occurrence of the specified time.
        let mut date_time =
            OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        if time < date_time.time() {
            date_time += Duration::days(1);
        }
        date_time = date_time.replace_time(time);

        date_time
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
    Confirm,
    Back,
    MinuteCarousel,
    HourCarousel,
}

/// A text item list with infinite scrolling.
pub struct TextCarousel {
    velocity: ScrollVelocity,
    touch_point: Point<f64>,
    scroll_offset: f64,

    items: Vec<String>,

    scale: f64,
    dirty: bool,
}

impl TextCarousel {
    fn new(items: Vec<String>) -> Self {
        Self {
            items,
            dirty: true,
            scale: 1.,
            scroll_offset: Default::default(),
            touch_point: Default::default(),
            velocity: Default::default(),
        }
    }

    /// Render the carousel.
    fn draw(&mut self, scale: f64, canvas: &Canvas, render_config: &RenderConfig, rect: Rect) {
        self.dirty = false;

        // Update scroll offset if scale has changed.
        if self.scale != scale {
            self.scroll_offset *= scale / self.scale;
        }
        self.scale = scale;

        // Animate scroll velocity.
        self.velocity.apply(&render_config.input_config, &mut self.scroll_offset);

        // Ensure offset is correct in case scale changed.
        self.clamp_scroll_offset();

        // Draw wheel background.
        canvas.draw_rect(rect, &render_config.button_paint);

        // Set clipping mask to cut off partially visible elements.
        canvas.save();
        canvas.clip_rect(rect, None, Some(false));

        // Calculate visible carousel items and sub-item offsets.
        let item_count = self.items.len() as isize;
        let item_height = CAROUSEL_ITEM_SIZE * scale;
        let index = (-self.scroll_offset / item_height).floor() as isize - 1;
        let offset = -((item_height - self.scroll_offset % item_height) % item_height) as f32;
        let visible_count = if offset == 0. { 3 } else { 4 };

        // Draw all visible items.
        for i in 0..visible_count {
            // Calculate index, wrapping at array boundaries.
            let index = (index + i as isize).rem_euclid(item_count) as usize;

            // Configure text rendering style.
            let mut paragraph_style = ParagraphStyle::new();
            paragraph_style.set_text_style(&render_config.text_style);
            paragraph_style.set_text_align(TextAlign::Center);

            // Perform text shaping and layout.
            let mut builder = ParagraphBuilder::new(&paragraph_style, &render_config.fonts);
            builder.add_text(&self.items[index]);
            let mut paragraph = builder.build();
            paragraph.layout(rect.right - rect.left);

            // Calculate item's position.
            let y_offset = (item_height as f32 - paragraph.height()) / 2.;
            let mut point = Point::new(rect.left, rect.top);
            point.y += i as f32 * item_height as f32 + offset + y_offset;

            paragraph.paint(canvas, point);
        }

        // Reset clipping mask.
        canvas.restore();
    }

    fn dirty(&self) -> bool {
        self.dirty || self.velocity.is_moving()
    }

    /// Handle touch press.
    fn touch_down(&mut self, physical_point: Point<f64>) {
        // Cancel velocity when a new touch sequence starts.
        self.velocity.set(0.);

        self.touch_point = physical_point;
    }

    /// Handle touch motion.
    fn touch_motion(&mut self, physical_point: Point<f64>) {
        // Update touch position.
        let old_point = mem::replace(&mut self.touch_point, physical_point);

        // Calculate current scroll velocity.
        let delta = self.touch_point.y - old_point.y;
        self.velocity.set(delta);

        // Immediately start moving the tabs list.
        let old_offset = self.scroll_offset;
        self.scroll_offset += delta;
        self.clamp_scroll_offset();
        self.dirty |= self.scroll_offset != old_offset;
    }

    /// Clamp alarm list viewport offset.
    ///
    /// While the scroll offset is in theory unbound due to remainders, clamping
    /// it regularly will avoid excessive floating point precision errors.
    fn clamp_scroll_offset(&mut self) {
        let old_offset = self.scroll_offset;
        self.scroll_offset %= self.max_scroll_offset();
        self.dirty |= old_offset != self.scroll_offset;
    }

    /// Get maximum alarm list viewport offset.
    fn max_scroll_offset(&self) -> f64 {
        let item_height = CAROUSEL_ITEM_SIZE * self.scale;
        self.items.len() as f64 * item_height
    }

    /// Get the selected value.
    fn value(&self) -> u8 {
        // Calculate index based on current offset.
        let item_height = CAROUSEL_ITEM_SIZE * self.scale;
        let index = (-self.scroll_offset / item_height).round() as isize;
        let index = index.rem_euclid(self.items.len() as isize) as usize;

        // Parse item as number.
        str::parse(&self.items[index]).unwrap()
    }

    /// Scroll to the item at the specified index.
    fn scroll_to(&mut self, index: usize) {
        let item_height = CAROUSEL_ITEM_SIZE * self.scale;
        self.scroll_offset = -item_height * index as f64;
        self.dirty = true;
    }
}
