use clap::Parser;
use log::{debug, error, info, LevelFilter};
use std::sync::{Arc, Mutex};
use anyhow::Result;

mod api;
mod browser;
mod event;
mod platform;
mod templates;

use crate::{
    browser::BrowserEngine,
    event::EventSystem,
    platform::Platform,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "A craftsperson's browser", long_about = None)]
struct Args {
    /// URL to load
    #[arg(short, long)]
    url: Option<String>,

    /// Run in headless mode
    #[arg(short = 'H', long)]
    headless: bool,

    /// Event broker URL
    #[arg(short, long)]
    broker_url: Option<String>,

    /// Number of tabs to open
    #[arg(long)]
    tabs: Option<usize>,

    /// Enable event recording
    #[arg(long)]
    record: bool,

    /// Path to save recorded events
    #[arg(long)]
    record_path: Option<String>,

    /// Path to replay events from
    #[arg(long)]
    replay: Option<String>,

    /// Speed multiplier for replay
    #[arg(long)]
    replay_speed: Option<f32>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::new()
        .filter_level(LevelFilter::Debug)
        .init();

    info!("Starting Tinker Workshop...");
    
    // Detect platform
    let platform = Platform::current();
    info!("Running on {:?} platform", platform);

    // Parse command line arguments
    let args = Args::parse();

    // Initialize event system if broker URL is specified
    let events = if let Some(broker_url) = args.broker_url.as_ref() {
        let events = EventSystem::new(broker_url, "tinker-browser");
        Some(Arc::new(Mutex::new(events)))
    } else {
        None
    };

    // Create browser instance with platform-specific configuration
    let mut browser = BrowserEngine::new(
        args.headless,
        events.clone(),
        args.url.clone(),
    )?;

    // Create initial tab
    let tab_id = browser.create_tab("about:blank")?;

    // Create additional tabs if requested
    if let Some(num_tabs) = args.tabs {
        info!("Creating {} tabs", num_tabs);
        for i in 1..num_tabs {
            browser.create_tab("about:blank")?;
            info!("Created new tab {}", i);
        }
    }

    // Load URL if provided
    if let Some(url) = args.url {
        browser.navigate(&url)?;
        info!("Navigating to: {}", url);
    }

    // Connect to event system after browser is initialized
    if let Some(ref events) = events {
        if let Ok(mut events) = events.lock() {
            if let Err(e) = events.connect() {
                error!("Failed to connect to event broker: {}. Continuing without event system.", e);
            } else {
                // Subscribe to all browser events using wildcard
                if let Err(e) = events.subscribe("browser/#") {
                    error!("Failed to subscribe to events: {}. Continuing without event subscription.", e);
                } else {
                    info!("Connected to event broker and subscribed to events");
                }
            }
        }
    }

    // Start recording if enabled
    if args.record {
        if let Some(path) = args.record_path.as_deref() {
            browser.start_recording(path);
            info!("Recording will be saved to {}", path);
        } else {
            return Err("--record-path is required when --record is specified".into());
        }
    }

    // Start replay if enabled
    if let Some(path) = args.replay {
        browser.load_recording(&path)?;
        if let Some(speed) = args.replay_speed {
            browser.set_replay_speed(speed)?;
        }
        browser.start_replay()?;
        info!("Replaying events from {}", path);
    }

    // Start event loop
    info!("Starting browser event loop");
    browser.run()?;

    Ok(())
}
