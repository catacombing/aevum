use std::cell::Cell;
use std::collections::HashMap;

use alarm::{Alarms, Event, Subscriber};
use gtk4::gdk::Display;
use gtk4::gio::ApplicationFlags;
use gtk4::glib::char::Char;
use gtk4::glib::{ExitCode, MainContext, OptionArg, OptionFlags};
use gtk4::prelude::*;
use gtk4::{
    AlertDialog, Align, Application, ApplicationWindow, Button, CssProvider, Label, Orientation,
    ScrolledWindow, Window,
};
use rezz::Alarm;
use time::macros::format_description;
use time::{Duration, OffsetDateTime, UtcOffset};
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::navigation::{Navigator, Page};
use crate::new_alarm::NewAlarmPage;
use crate::ringing_alarm::RingingAlarmPage;

pub mod navigation;
mod new_alarm;
mod ringing_alarm;

/// Wayland application ID.
const APP_ID: &str = "catacomb.Alarm";

#[tokio::main]
async fn main() -> ExitCode {
    // Setup application.
    let application = Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    // Add CLI flags.
    application.add_main_option(
        "daemon",
        Char::from(b'd'),
        OptionFlags::NONE,
        OptionArg::None,
        "Launch application in the background",
        None,
    );

    // Load CSS.
    application.connect_startup(|_| {
        // Create stylesheet.
        let provider = CssProvider::new();
        provider.load_from_data(include_str!("../style.css"));

        // Apply stylesheet to the application.
        gtk4::style_context_add_provider_for_display(
            &Display::default().expect("connect to display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });

    // Create channel for spawning new windows.
    let (new_window_tx, new_window_rx) = mpsc::channel(16);

    // Initialize state shared across all windows.
    let state = AlarmGtk::new(&application, new_window_rx);

    // Handle CLI from any instance.
    let state = Cell::new(Some(state));
    application.connect_command_line(move |_app, cmdline| {
        let daemon_mode = cmdline.options_dict().contains("daemon");
        match state.take() {
            // Start event loop on first run.
            Some(state) => state.start_master(daemon_mode),
            // Only launch windows if daemon mode flag wasn't passed.
            None if !daemon_mode => {
                let _ = new_window_tx.try_send(());
            },
            None => eprintln!("Error: Daemon mode already running"),
        }

        ExitCode::SUCCESS
    });

    // Run application.
    application.run()
}

/// Main application state.
struct AlarmGtk {
    windows: HashMap<u32, Overview>,
    window_close_tx: Sender<u32>,
    window_close_rx: Receiver<u32>,
    new_window_rx: Receiver<()>,
    app: Application,
}

impl AlarmGtk {
    fn new(app: &Application, new_window_rx: Receiver<()>) -> Self {
        let (window_close_tx, window_close_rx) = mpsc::channel(256);
        Self {
            window_close_tx,
            window_close_rx,
            new_window_rx,
            app: app.clone(),
            windows: Default::default(),
        }
    }

    /// Start the master window.
    ///
    /// This will always start the event loop and open a new window if not
    /// launched in daemon mode.
    fn start_master(mut self, daemon_mode: bool) {
        let mut daemon_guard = None;
        if daemon_mode {
            // Prevent automatic exit when created without any windows.
            daemon_guard = Some(self.app.hold());
        } else {
            // Spawn initial window when not running in daemon mode.
            self.open_window();
        }

        // Run main event loop.
        MainContext::default().spawn_local(async move {
            self.listen().await;

            // Release the GIO application guard, closing the application.
            daemon_guard.take();
        });
    }

    /// Handle events.
    async fn listen(mut self) {
        // Subscribe to DBus events.
        let mut subscriber = match Subscriber::new().await {
            Ok(subscriber) => subscriber,
            Err(err) => {
                if self.windows.is_empty() {
                    eprintln!("{err}");
                } else {
                    show_error(err.to_string());
                }
                return;
            },
        };

        // If we're not running in daemon mode, seed view with initial alarms.
        self.update_alarms(subscriber.alarms());

        loop {
            tokio::select! {
                Some(id) = self.window_close_rx.recv() => {
                    self.windows.remove(&id);
                },
                _ = self.new_window_rx.recv() => self.open_window(),
                Some(event) = subscriber.next() => match event {
                    // Handle new/removed alarms.
                    Event::AlarmsChanged(alarms) => self.update_alarms(&alarms),
                    // Handle ringing alarms.
                    Event::Ring(alarm) => {
                        // Ensure at least one window is open.
                        if self.windows.is_empty() {
                            self.open_window();
                        }

                        // Ring any available window.
                        if let Some(window) = self.windows.values_mut().next() {
                            window.ring(alarm).await;
                        }
                    },
                }
            }
        }
    }

    /// Update the UI's alarms.
    fn update_alarms(&mut self, alarms: &[Alarm]) {
        for window in self.windows.values_mut() {
            window.update(alarms);
        }
    }

    /// Open the GTK4 UI.
    fn open_window(&mut self) {
        // Configure window settings.
        let window = ApplicationWindow::builder().application(&self.app).title("Alarm").build();

        // Setup page navigation.
        let navigator = Navigator::new();
        window.set_child(Some(navigator.widget()));

        // Add alarm creation page.
        let new_alarm_page = NewAlarmPage::new(navigator.clone());
        navigator.add(&new_alarm_page);

        // Add ringing alarm page.
        let ringing_alarm_page = RingingAlarmPage::new(navigator.clone());
        navigator.add(&ringing_alarm_page);

        // Add landing page.
        let overview = Overview::new(navigator.clone(), new_alarm_page, ringing_alarm_page);
        navigator.add(&overview);

        // Show window.
        navigator.show(Overview::id());
        window.present();

        // Clear UI once window is destroyed.
        let exit_tx = self.window_close_tx.clone();
        window.connect_destroy(move |window| {
            let _ = exit_tx.try_send(window.id());
        });

        self.windows.insert(window.id(), overview);
    }
}

/// Alarm overview and landing page.
pub struct Overview {
    ringing_alarm_page: RingingAlarmPage,
    alarms: ScrolledWindow,
    container: gtk4::Box,
}

impl Overview {
    fn new(
        navigator: Navigator,
        new_alarm_page: NewAlarmPage,
        ringing_alarm_page: RingingAlarmPage,
    ) -> Self {
        let container = gtk4::Box::new(Orientation::Vertical, 0);
        container.set_valign(Align::End);

        // Create alarms container.
        let alarms = ScrolledWindow::new();
        container.append(&alarms);

        // Button to create new alarms.
        let new_button = Button::with_label("Add Alarm");
        new_button.set_margin_top(25);
        new_button.set_margin_end(25);
        new_button.set_margin_bottom(25);
        new_button.set_margin_start(25);
        container.append(&new_button);

        // Handle new alarm button press.
        new_button.connect_clicked(move |_| {
            new_alarm_page.reset();
            navigator.show(NewAlarmPage::id());
        });

        Self { container, alarms, ringing_alarm_page }
    }

    /// Update the view with new alarms.
    fn update(&mut self, alarms: &[Alarm]) {
        // Create new alarms container.
        let container = gtk4::Box::new(Orientation::Vertical, 0);
        for alarm in alarms {
            container.append(&Self::alarm_components(alarm));
        }

        // Create scroll box.
        let scroll = ScrolledWindow::new();
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&container));

        // Put scrollbox at bottom by default.
        scroll.vadjustment().connect_upper_notify(|adj| adj.set_value(adj.upper()));

        // Swap containers.
        self.container.remove(&self.alarms);
        self.container.prepend(&scroll);
        self.alarms = scroll;
    }

    /// Ring an alarm.
    async fn ring(&mut self, alarm: Alarm) {
        self.ringing_alarm_page.ring(alarm).await;
    }

    /// Get the GTK components for an alarm.
    fn alarm_components(alarm: &Alarm) -> gtk4::Box {
        // Convert unix time to local time.
        let utc_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        let time = OffsetDateTime::UNIX_EPOCH + Duration::seconds(alarm.unix_time);
        let local_time = time.to_offset(utc_offset);

        let container = gtk4::Box::new(Orientation::Horizontal, 0);
        container.set_margin_start(50);
        container.set_margin_top(10);
        container.set_margin_end(50);
        container.set_margin_bottom(10);

        // Create vertical container to show date below time.
        let datetime_container = gtk4::Box::new(Orientation::Vertical, 0);
        datetime_container.set_hexpand(true);
        container.append(&datetime_container);

        // Add alarm's time.
        let time_format = format_description!("[hour]:[minute]");
        let time_str = local_time.format(&time_format).unwrap();
        let time_label = Label::new(Some(&time_str));
        time_label.add_css_class("overview-alarm-time");
        time_label.set_halign(Align::Start);
        datetime_container.append(&time_label);

        // Add alarms date.
        let date_format = format_description!("[year]-[month]-[day]");
        let date_str = local_time.format(&date_format).unwrap();
        let date_label = Label::new(Some(&date_str));
        date_label.add_css_class("overview-alarm-date");
        date_label.set_halign(Align::Start);
        datetime_container.append(&date_label);

        // Add button to dismiss alarm.
        let button = Button::from_icon_name("edit-delete");
        button.add_css_class("overview-alarm-button");
        container.append(&button);

        // Remove alarm on button press.
        let id = alarm.id.clone();
        button.connect_clicked(move |_| {
            let id = id.clone();
            MainContext::default().spawn(async move {
                if let Err(err) = Alarms.remove(id.clone()).await {
                    show_error(err.to_string());
                }
            });
        });

        container
    }
}

impl Page<gtk4::Box> for Overview {
    fn id() -> &'static str {
        "overview"
    }

    fn widget(&self) -> &gtk4::Box {
        &self.container
    }
}

/// Display an error message in a new window.
pub fn show_error(message: String) {
    let alert = AlertDialog::builder().message(message).build();
    alert.show(None::<&Window>);
}
