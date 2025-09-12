//! Alarm creation UI.

use std::mem;

use alarm::Alarms;
use rezz::Alarm;
use skia_safe::textlayout::{ParagraphBuilder, ParagraphStyle, TextAlign};
use skia_safe::{Canvas, Rect};
use time::{Duration, OffsetDateTime, Time};
use tracing::error;
use uuid::Uuid;

use crate::config::Input;
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

/// Size of the hour/time separator colons at scale 1.
const COLON_SIZE: f64 = 6.;

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

        // Draw text showing delta to alarm time.
        let delta_rect = Self::delta_text_rect(self.size, self.scale);
        self.draw_centered_text(canvas, render_config, delta_rect, &self.delta_text());

        // Draw time selection wheels.
        let hour_rect = Self::hour_carousel_rect(self.size, scale);
        self.hour_carousel.draw(scale, canvas, render_config, hour_rect);
        let minute_rect = Self::minute_carousel_rect(self.size, scale);
        self.minute_carousel.draw(scale, canvas, render_config, minute_rect);

        // Draw hour/minute separator colons.
        let (colon_rect_top, colon_rect_bottom) = Self::colon_rects(self.size, scale);
        canvas.draw_rect(colon_rect_top, &render_config.text_paint);
        canvas.draw_rect(colon_rect_bottom, &render_config.text_paint);

        // Draw the cancel creation button.
        let back_rect = Self::back_button_rect(self.size, scale);
        canvas.draw_rect(back_rect, &render_config.button_paint);
        Icon::Back.draw(canvas, scale, &render_config.icon_paint, back_rect);

        // Draw quick-set buttons.

        let quick_rect_1 = Self::quick_action_rect_1(self.size, scale);
        canvas.draw_rect(quick_rect_1, &render_config.button_paint);
        let quick_text_1 = self.quick_text(render_config.input_config.quick_minutes_1);
        self.draw_centered_text(canvas, render_config, quick_rect_1, &quick_text_1);

        let quick_rect_2 = Self::quick_action_rect_2(self.size, scale);
        canvas.draw_rect(quick_rect_2, &render_config.button_paint);
        let quick_text_2 = self.quick_text(render_config.input_config.quick_minutes_2);
        self.draw_centered_text(canvas, render_config, quick_rect_2, &quick_text_2);

        // Draw the confirm creation button.
        let confirm_rect = Self::confirm_button_rect(self.size, scale);
        canvas.draw_rect(confirm_rect, &render_config.button_paint);
        Icon::Confirm.draw(canvas, scale, &render_config.icon_paint, confirm_rect);
    }

    /// Draw text centered within a rectangle.
    fn draw_centered_text(
        &self,
        canvas: &Canvas,
        render_config: &RenderConfig,
        rect: Rect,
        text: &str,
    ) {
        // Setup text style for alarm delta.
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_style(&render_config.text_style);
        paragraph_style.set_text_align(TextAlign::Center);

        // Create and layout the paragraph.
        let mut paragraph_builder = ParagraphBuilder::new(&paragraph_style, &render_config.fonts);
        paragraph_builder.add_text(text);
        let mut paragraph = paragraph_builder.build();
        paragraph.layout(rect.right - rect.left);

        // Center paragraph vertically inside its rect.
        let delta_y_offset = (rect.bottom - rect.top - paragraph.height()) / 2.;
        paragraph.paint(canvas, Point::new(rect.left, rect.top + delta_y_offset));
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
        let quick_rect_1 = Self::quick_action_rect_1(self.size, self.scale);
        let quick_rect_2 = Self::quick_action_rect_2(self.size, self.scale);
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
        } else if rect_contains(quick_rect_1, point) {
            self.touch_state.action = TouchAction::QuickAction1;
        } else if rect_contains(quick_rect_2, point) {
            self.touch_state.action = TouchAction::QuickAction2;
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
    pub fn touch_up(&mut self, input_config: &Input) -> WindowTouchAction {
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
            // Add 90 minutes to the current alarm.
            TouchAction::QuickAction1 => self.add_minutes(input_config.quick_minutes_1),
            // Add 8 hours to the current alarm.
            TouchAction::QuickAction2 => self.add_minutes(input_config.quick_minutes_2),
            TouchAction::MinuteCarousel => self.minute_carousel.touch_up(),
            TouchAction::HourCarousel => self.hour_carousel.touch_up(),
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

    /// Physical rectangle of the left quick action button.
    fn quick_action_rect_1(size: Size<f32>, scale: f64) -> Rect {
        let back_rect = Self::back_button_rect(size, scale);
        let button_padding = (BUTTON_PADDING * scale) as f32;
        let space = (CAROUSEL_SPACE * scale) as f32;

        let height = back_rect.bottom - back_rect.top;
        let y = back_rect.top - button_padding - height;

        let left = (OUTSIDE_PADDING * scale) as f32;
        let right = (size.width - space) / 2.;

        Rect::new(left, y, right, y + height)
    }

    /// Physical rectangle of the right quick action button.
    fn quick_action_rect_2(size: Size<f32>, scale: f64) -> Rect {
        let back_rect = Self::back_button_rect(size, scale);
        let button_padding = (BUTTON_PADDING * scale) as f32;
        let space = (CAROUSEL_SPACE * scale) as f32;

        let height = back_rect.bottom - back_rect.top;
        let y = back_rect.top - button_padding - height;

        let left = (size.width + space) / 2.;
        let right = size.width - (OUTSIDE_PADDING * scale) as f32;

        Rect::new(left, y, right, y + height)
    }

    /// Physical rectangle of the time delta text.
    fn delta_text_rect(size: Size<f32>, scale: f64) -> Rect {
        let hour_rect = Self::hour_carousel_rect(size, scale);
        let padding = (BUTTON_PADDING * scale) as f32;
        let height = (BUTTON_HEIGHT * scale) as f32;

        let y = hour_rect.top - padding - height;

        Rect::new(0., y, size.width, y + height)
    }

    /// Physical rectangle of the hour selection wheel.
    fn hour_carousel_rect(size: Size<f32>, scale: f64) -> Rect {
        let quick_rect = Self::quick_action_rect_1(size, scale);
        let button_padding = (BUTTON_PADDING * scale) as f32;
        let item_size = (CAROUSEL_ITEM_SIZE * scale) as f32;
        let space = (CAROUSEL_SPACE * scale) as f32;

        let height = item_size * 3.;
        let y = quick_rect.top - button_padding - height;
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

    /// Physical rectangles of the hour/minute separator colons.
    fn colon_rects(size: Size<f32>, scale: f64) -> (Rect, Rect) {
        let hour_rect = Self::hour_carousel_rect(size, scale);
        let colon_size = (COLON_SIZE * scale) as f32;

        let x = size.width / 2. - colon_size / 2.;
        let hour_center = hour_rect.top + (hour_rect.bottom - hour_rect.top) / 2.;
        let top_y = hour_center - 1.5 * colon_size;
        let bottom_y = hour_center + 0.5 * colon_size;

        let top = Rect::new(x, top_y, x + colon_size, top_y + colon_size);
        let bottom = Rect::new(x, bottom_y, x + colon_size, bottom_y + colon_size);

        (top, bottom)
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

    // Text label for a quick action interval.
    fn quick_text(&self, minutes: u16) -> String {
        if minutes % 60 == 0 {
            format!("+ {} Hours", minutes / 60)
        } else {
            format!("+ {} Minutes", minutes)
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

    /// Add `interval` minutes to the current alarm.
    fn add_minutes(&mut self, interval: u16) {
        let minutes = self.minute_carousel.value() as usize;
        let hours = self.hour_carousel.value() as usize;

        let new_minutes = (minutes + interval as usize) % 60;
        let new_hours = hours + (minutes + interval as usize) / 60;

        self.minute_carousel.scroll_to(new_minutes / 5);
        self.hour_carousel.scroll_to(new_hours);
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
    QuickAction1,
    QuickAction2,
}

/// A text item list with infinite scrolling.
pub struct TextCarousel {
    velocity: ScrollVelocity,
    touch_point: Point<f64>,
    touch_active: bool,
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
            touch_active: Default::default(),
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

        // Snap scroll offset to nearest item after drag completion.
        if !self.velocity.is_moving() && !self.touch_active {
            self.scroll_offset = self.rounded_offset();
        }

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
        self.dirty
            || self.velocity.is_moving()
            || (!self.touch_active && self.scroll_offset != self.rounded_offset())
    }

    /// Handle touch press.
    fn touch_down(&mut self, physical_point: Point<f64>) {
        // Cancel velocity when a new touch sequence starts.
        self.velocity.set(0.);

        self.touch_point = physical_point;
        self.touch_active = true;
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

    /// Handle touch release.
    fn touch_up(&mut self) {
        self.touch_active = false;
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

    /// Get the nearest item offset.
    fn rounded_offset(&self) -> f64 {
        let item_height = CAROUSEL_ITEM_SIZE * self.scale;

        let remainder = self.scroll_offset % item_height;
        let mut offset = self.scroll_offset - remainder;

        if remainder.abs() >= item_height / 2. {
            offset += item_height.copysign(self.scroll_offset);
        }

        offset
    }
}
