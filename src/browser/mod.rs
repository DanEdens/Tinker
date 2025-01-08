//! Browser engine implementation

use std::{
    sync::{Arc, Mutex, mpsc::channel},
    time::{Duration, Instant},
};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    dpi::LogicalSize,
};
use wry::{WebView, WebViewBuilder};
use tracing::{debug, info};

use crate::{
    event::{BrowserEvent, EventSystem},
    browser::{
        tabs::TabManager,
        tab_ui::{TabBar, TabCommand},
        replay::{EventRecorder, EventPlayer},
        event_viewer::EventViewer,
    },
};

mod tabs;
mod event_viewer;
mod tab_ui;
mod replay;

use tabs::TabManager;
use event_viewer::EventViewer;
use tab_ui::{TabBar, TabCommand};
use replay::{EventRecorder, EventPlayer};

#[derive(Debug)]
enum BrowserCommand {
    Navigate { url: String },
    CreateTab { url: String },
    CloseTab { id: usize },
    SwitchTab { id: usize },
    RecordEvent { event: BrowserEvent },
    PlayEvent { event: BrowserEvent },
}

pub struct BrowserEngine {
    headless: bool,
    events: Option<Arc<Mutex<EventSystem>>>,
    player: Arc<Mutex<EventPlayer>>,
    recorder: Arc<Mutex<EventRecorder>>,
    event_viewer: Arc<Mutex<EventViewer>>,
    tabs: Arc<Mutex<TabManager>>,
    tab_bar: Option<TabBar>,
    content_view: Option<WebView>,
}

impl BrowserEngine {
    pub fn new(headless: bool, events: Option<Arc<Mutex<EventSystem>>>) -> Self {
        BrowserEngine {
            headless,
            events,
            player: Arc::new(Mutex::new(EventPlayer::default())),
            recorder: Arc::new(Mutex::new(EventRecorder::default())),
            event_viewer: Arc::new(Mutex::new(EventViewer::default())),
            tabs: Arc::new(Mutex::new(TabManager::default())),
            tab_bar: None,
            content_view: None,
        }
    }

    pub fn navigate(&self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        info!("Navigating to: {}", url);

        // Update the tab URL first
        if let Ok(mut tabs) = self.tabs.lock() {
            if let Some(tab) = tabs.get_active_tab_mut() {
                tab.url = url.to_string();
            }
        }

        // Then update the WebView
        if let Some(view) = &self.content_view {
            view.load_url(url);
        }

        // Finally, emit the navigation event
        if let Some(events) = &self.events {
            if let Ok(mut events) = events.lock() {
                events.publish(BrowserEvent::Navigation {
                    url: url.to_string(),
                })?;
            }
        }

        Ok(())
    }

    pub fn get_active_tab(&self) -> Option<String> {
        if let Ok(tabs) = self.tabs.lock() {
            tabs.get_active_tab().map(|tab| tab.url.clone())
        } else {
            None
        }
    }

    pub fn init_events(&mut self, broker_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut events = EventSystem::new(broker_url, "tinker-browser");
        events.connect()?;
        self.events = Some(Arc::new(Mutex::new(events)));
        Ok(())
    }

    pub fn start_recording(&self) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.start();
        }
    }

    pub fn stop_recording(&self) {
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.stop();
        }
    }

    pub fn save_recording(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(recorder) = self.recorder.lock() {
            recorder.save(path)?;
        }
        Ok(())
    }

    pub fn load_recording(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(mut player) = self.player.lock() {
            player.load(path)?;
        }
        Ok(())
    }

    pub fn start_replay(&self) {
        if let Ok(mut player) = self.player.lock() {
            player.start();
        }
    }

    pub fn set_replay_speed(&self, speed: f32) {
        if let Ok(mut player) = self.player.lock() {
            player.set_speed(speed);
        }
    }

    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Starting browser engine...");

        let event_loop = EventLoop::new();
        
        // Create the main window
        let window_builder = WindowBuilder::new()
            .with_title("Tinker Browser")
            .with_inner_size(LogicalSize::new(1024.0, 768.0));

        let window = window_builder.build(&event_loop)?;

        // Create the content view
        if let Ok(tabs) = self.tabs.lock() {
            if let Some(active_tab) = tabs.get_active_tab() {
                let content_view = WebViewBuilder::new(&window)
                    .with_url(&active_tab.url)?
                    .build()?;
                self.content_view = Some(content_view);
            }
        }

        // Create channels for browser commands
        let (cmd_tx, cmd_rx) = channel::<BrowserCommand>();

        // Create the tab bar if not in headless mode
        if !self.headless {
            // Create the tab bar with the command sender
            let tab_cmd_tx = cmd_tx.clone();
            let tab_bar = TabBar::new(&window, move |cmd| {
                match cmd {
                    TabCommand::Create { url } => {
                        let _ = tab_cmd_tx.send(BrowserCommand::CreateTab { url });
                    }
                    TabCommand::Close { id } => {
                        let _ = tab_cmd_tx.send(BrowserCommand::CloseTab { id });
                    }
                    TabCommand::Switch { id } => {
                        let _ = tab_cmd_tx.send(BrowserCommand::SwitchTab { id });
                    }
                }
            })?;
            
            // Add existing tabs to the UI
            if let Ok(tabs) = self.tabs.lock() {
                for tab in tabs.get_all_tabs() {
                    tab_bar.add_tab(tab.id, &tab.title, &tab.url);
                }
                if let Some(active_tab) = tabs.get_active_tab() {
                    tab_bar.set_active_tab(active_tab.id);
                }
            }
            self.tab_bar = Some(tab_bar);
        }

        // Set up event handling
        let headless = self.headless;
        let events = self.events.clone();
        let player = self.player.clone();
        let recorder = self.recorder.clone();
        let event_viewer = self.event_viewer.clone();
        let tabs = self.tabs.clone();
        let tab_bar = self.tab_bar.clone();
        let mut last_replay_time = Instant::now();
        
        // Store command sender for use in event loop
        let cmd_tx_for_events = cmd_tx.clone();

        // Move content_view into the event loop
        let content_view = self.content_view.take();

        event_loop.run(move |event, _, control_flow| {
            *control_flow = if headless {
                ControlFlow::Exit
            } else {
                ControlFlow::Wait
            };

            // Check for replay events
            if let Ok(mut player) = player.lock() {
                if let Some(event) = player.next_event() {
                    let now = Instant::now();
                    if now.duration_since(last_replay_time) >= Duration::from_millis(100) {
                        match &event {
                            BrowserEvent::Navigation { url } => {
                                let _ = cmd_tx_for_events.send(BrowserCommand::Navigate { url: url.clone() });
                            }
                            BrowserEvent::TabCreated { .. } => {
                                let _ = cmd_tx_for_events.send(BrowserCommand::CreateTab { url: "about:blank".to_string() });
                            }
                            BrowserEvent::TabClosed { id } => {
                                let _ = cmd_tx_for_events.send(BrowserCommand::CloseTab { id: *id });
                            }
                            BrowserEvent::TabSwitched { id } => {
                                let _ = cmd_tx_for_events.send(BrowserCommand::SwitchTab { id: *id });
                            }
                            _ => {}
                        }
                        if let Ok(mut recorder) = recorder.lock() {
                            recorder.record_event(event.clone());
                        }
                        if let Ok(mut event_viewer) = event_viewer.lock() {
                            event_viewer.add_event(event);
                        }
                        last_replay_time = now;
                    }
                }
            }

            // Handle browser commands
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    BrowserCommand::Navigate { url } => {
                        if let Some(ref view) = content_view {
                            view.load_url(&url);
                        }
                    }
                    BrowserCommand::CreateTab { url } => {
                        if let Ok(mut tabs) = tabs.lock() {
                            let id = tabs.create_tab(url.clone());
                            if let Some(ref bar) = tab_bar {
                                bar.add_tab(id, "New Tab", &url);
                                bar.set_active_tab(id);
                            }
                            if let Some(ref view) = content_view {
                                view.load_url(&url);
                            }
                            if let Some(events) = &events {
                                if let Ok(mut events) = events.lock() {
                                    let _ = events.publish(BrowserEvent::TabCreated { id });
                                }
                            }
                        }
                    }
                    BrowserCommand::CloseTab { id } => {
                        if let Ok(mut tabs) = tabs.lock() {
                            if tabs.close_tab(id) {
                                if let Some(ref bar) = tab_bar {
                                    bar.remove_tab(id);
                                    if let Some(active_tab) = tabs.get_active_tab() {
                                        bar.set_active_tab(active_tab.id);
                                        if let Some(ref view) = content_view {
                                            view.load_url(&active_tab.url);
                                        }
                                    }
                                }
                                if let Some(events) = &events {
                                    if let Ok(mut events) = events.lock() {
                                        let _ = events.publish(BrowserEvent::TabClosed { id });
                                    }
                                }
                            }
                        }
                    }
                    BrowserCommand::SwitchTab { id } => {
                        if let Ok(mut tabs) = tabs.lock() {
                            if tabs.switch_to_tab(id) {
                                if let Some(ref bar) = tab_bar {
                                    bar.set_active_tab(id);
                                }
                                if let Some(active_tab) = tabs.get_active_tab() {
                                    if let Some(ref view) = content_view {
                                        view.load_url(&active_tab.url);
                                    }
                                }
                                if let Some(events) = &events {
                                    if let Ok(mut events) = events.lock() {
                                        let _ = events.publish(BrowserEvent::TabSwitched { id });
                                    }
                                }
                            }
                        }
                    }
                    BrowserCommand::RecordEvent { event } => {
                        if let Ok(mut recorder) = recorder.lock() {
                            recorder.record_event(event.clone());
                        }
                        if let Ok(mut event_viewer) = event_viewer.lock() {
                            event_viewer.add_event(event);
                        }
                    }
                    BrowserCommand::PlayEvent { event } => {
                        match &event {
                            BrowserEvent::Navigation { url } => {
                                if let Some(ref view) = content_view {
                                    view.load_url(url);
                                }
                            }
                            BrowserEvent::TabCreated { .. } => {
                                if let Ok(mut tabs) = tabs.lock() {
                                    let id = tabs.create_tab("about:blank".to_string());
                                    if let Some(ref bar) = tab_bar {
                                        bar.add_tab(id, "New Tab", "about:blank");
                                        bar.set_active_tab(id);
                                    }
                                }
                            }
                            BrowserEvent::TabClosed { id } => {
                                if let Ok(mut tabs) = tabs.lock() {
                                    if tabs.close_tab(*id) {
                                        if let Some(ref bar) = tab_bar {
                                            bar.remove_tab(*id);
                                        }
                                    }
                                }
                            }
                            BrowserEvent::TabSwitched { id } => {
                                if let Ok(mut tabs) = tabs.lock() {
                                    if tabs.switch_to_tab(*id) {
                                        if let Some(ref bar) = tab_bar {
                                            bar.set_active_tab(*id);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => (),
            }
        });

        #[allow(unreachable_code)]
        Ok(())
    }

    pub fn create_tab(&mut self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(mut tabs) = self.tabs.lock() {
            tabs.create_tab(url.to_string());
            Ok(())
        } else {
            Err("Failed to lock tabs".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_navigation() {
        let mut browser = BrowserEngine::new(false, None);
        
        // Create initial tab and get its URL
        let initial_url = {
            let mut tabs = browser.tabs.lock().unwrap();
            let id = tabs.create_tab("about:blank".to_string());
            tabs.get_active_tab().map(|tab| tab.url.clone()).unwrap()
        };
        assert_eq!(initial_url, "about:blank");

        // Navigate to the test URL
        browser.navigate("https://www.example.com").unwrap();

        // Verify the URL was updated
        let final_url = {
            let tabs = browser.tabs.lock().unwrap();
            tabs.get_active_tab().map(|tab| tab.url.clone()).unwrap()
        };
        assert_eq!(final_url, "https://www.example.com");
    }
}
