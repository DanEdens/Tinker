use tao::window::{Window, WindowBuilder};
use tao::dpi::LogicalSize;
use tracing::debug;
use std::sync::{Arc, Mutex};

pub enum TabCommand {
    Create { url: String },
    Close { id: usize },
    Switch { id: usize },
}

#[derive(Clone)]
pub struct TabBar {
    height: u32,
    tabs: Arc<Mutex<Vec<TabInfo>>>,
}

struct TabInfo {
    id: usize,
    title: String,
    url: String,
    active: bool,
}

impl TabBar {
    pub fn new<F>(window: &Window, command_handler: F) -> Result<Self, Box<dyn std::error::Error>>
    where
        F: Fn(TabCommand) + Send + 'static,
    {
        let height = 40;
        
        Ok(TabBar {
            height,
            tabs: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn add_tab(&self, id: usize, title: &str, url: &str) {
        if let Ok(mut tabs) = self.tabs.lock() {
            tabs.push(TabInfo {
                id,
                title: title.to_string(),
                url: url.to_string(),
                active: false,
            });
        }
    }

    pub fn remove_tab(&self, id: usize) {
        if let Ok(mut tabs) = self.tabs.lock() {
            if let Some(pos) = tabs.iter().position(|tab| tab.id == id) {
                tabs.remove(pos);
            }
        }
    }

    pub fn set_active_tab(&self, id: usize) {
        if let Ok(mut tabs) = self.tabs.lock() {
            for tab in tabs.iter_mut() {
                tab.active = tab.id == id;
            }
        }
    }

    pub fn get_height(&self) -> u32 {
        self.height
    }
} 
