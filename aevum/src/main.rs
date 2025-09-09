use std::{env, process, thread};

use alarm::{Event as AlarmEvent, Subscriber};
use calloop::channel::Event as ChannelEvent;
use calloop::{EventLoop, LoopHandle, channel};
use calloop_wayland_source::WaylandSource;
use configory::{Manager as ConfigManager, Options as ConfigOptions};
use smithay_client_toolkit::reexports::client::globals::{
    self, BindError, GlobalError, GlobalList,
};
use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer;
use smithay_client_toolkit::reexports::client::protocol::wl_touch::WlTouch;
use smithay_client_toolkit::reexports::client::{
    ConnectError, Connection, DispatchError, QueueHandle,
};
use tokio::runtime::Builder as RuntimeBuilder;
use tokio::task::LocalSet;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use crate::config::{Config, ConfigEventHandler};
use crate::ui::window::Window;
use crate::wayland::ProtocolStates;

mod config;
mod geometry;
mod ui;
mod wayland;

mod gl {
    #![allow(clippy::all, unsafe_op_in_unsafe_fn)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

#[tokio::main]
async fn main() {
    // Setup logging.
    let directives = env::var("RUST_LOG").unwrap_or("warn,aevum=info,configory=info".into());
    let env_filter = EnvFilter::builder().parse_lossy(directives);
    FmtSubscriber::builder().with_env_filter(env_filter).with_line_number(true).init();

    info!("Started Aevum");

    if let Err(err) = run().await {
        error!("[CRITICAL] {err}");
        process::exit(1);
    }
}

async fn run() -> Result<(), Error> {
    // Initialize Wayland connection.
    let connection = Connection::connect_to_env()?;
    let (globals, queue) = globals::registry_queue_init(&connection)?;

    let mut event_loop = EventLoop::try_new()?;
    let mut state =
        State::new(&event_loop.handle(), connection.clone(), &globals, queue.handle()).await?;

    // Insert wayland source into calloop loop.
    let wayland_source = WaylandSource::new(connection, queue);
    wayland_source.insert(event_loop.handle())?;

    // Start event loop.
    while !state.terminated {
        event_loop.dispatch(None, &mut state)?;
    }

    Ok(())
}

/// Application state.
struct State {
    protocol_states: ProtocolStates,

    pointer: Option<WlPointer>,
    touch: Option<WlTouch>,

    window: Window,
    config: Config,

    terminated: bool,

    _config_manager: ConfigManager,
}

impl State {
    async fn new(
        event_loop: &LoopHandle<'static, Self>,
        connection: Connection,
        globals: &GlobalList,
        queue: QueueHandle<Self>,
    ) -> Result<Self, Error> {
        let protocol_states = ProtocolStates::new(globals, &queue)?;

        // Initialize configuration state.
        let config_options = ConfigOptions::new("aevum").notify(true);
        let config_handler = ConfigEventHandler::new(event_loop);
        let config_manager = ConfigManager::with_options(&config_options, config_handler)?;
        let config = config_manager
            .get::<&str, Config>(&[])
            .inspect_err(|err| error!("Config error: {err}"))
            .ok()
            .flatten()
            .unwrap_or_default();

        // Create the Wayland window.
        let window = Window::new(&protocol_states, connection, queue, &config)?;

        // Listen for changes to pending alarms.
        Self::spawn_listener(event_loop)?;

        Ok(Self {
            protocol_states,
            config,
            window,
            _config_manager: config_manager,
            terminated: Default::default(),
            pointer: Default::default(),
            touch: Default::default(),
        })
    }

    /// Create a new thread to listen for DBus events.
    fn spawn_listener(event_loop: &LoopHandle<'static, Self>) -> Result<(), Error> {
        let rt = RuntimeBuilder::new_current_thread().enable_all().build().unwrap();
        let (alarms_tx, alarms_rx) = channel::channel();

        // Create a thread to listen for DBus alarm changes.
        //
        // This needs its own thread since `Subscriber` cannot be moved across threads
        // and Tokio does not support forcing local execution without a dedicated
        // thread.
        thread::spawn(move || {
            let local_set = LocalSet::new();
            local_set.spawn_local(async move {
                let mut subscriber = match Subscriber::new().await {
                    Ok(subscriber) => subscriber,
                    Err(err) => {
                        error!("Failed to create DBus listener: {err}");
                        return;
                    },
                };

                // Fill initial list of alarms.
                let alarms = subscriber.alarms().to_vec();
                let _ = alarms_tx.send(AlarmEvent::AlarmsChanged(alarms.into()));

                // Handle next alarm event.
                loop {
                    if let Some(event) = subscriber.next().await {
                        let event = match event {
                            AlarmEvent::AlarmsChanged(alarms) => {
                                AlarmEvent::AlarmsChanged(alarms.to_vec().into())
                            },
                            AlarmEvent::Ring(alarm) => AlarmEvent::Ring(alarm),
                        };
                        let _ = alarms_tx.send(event);
                    }
                }
            });
            rt.block_on(local_set);
        });

        // Process alarm change events.
        event_loop.insert_source(alarms_rx, |event, _, state| match event {
            ChannelEvent::Msg(AlarmEvent::AlarmsChanged(alarms)) => {
                state.window.set_alarms(alarms.to_vec());
            },
            ChannelEvent::Msg(AlarmEvent::Ring(alarm)) => state.window.ring(alarm),
            ChannelEvent::Closed => state.terminated = true,
        })?;

        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("Wayland protocol error for {0}: {1}")]
    WaylandProtocol(&'static str, #[source] BindError),
    #[error("{0}")]
    WaylandDispatch(#[from] DispatchError),
    #[error("{0}")]
    WaylandConnect(#[from] ConnectError),
    #[error("{0}")]
    WaylandGlobal(#[from] GlobalError),
    #[error("{0}")]
    EventLoop(#[from] calloop::Error),
    #[error("{0}")]
    Configory(#[from] configory::Error),
    #[error("{0}")]
    Glutin(#[from] glutin::error::Error),
    #[error("{0}")]
    Alarm(#[from] alarm::error::Error),
}

impl<T> From<calloop::InsertError<T>> for Error {
    fn from(err: calloop::InsertError<T>) -> Self {
        Self::EventLoop(err.error)
    }
}
