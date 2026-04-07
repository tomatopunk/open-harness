//! Event system (simplified placeholder)
//!
//! A simplified event system placeholder to avoid complex type issues.

/// Placeholder event trait
pub trait Event: Send + Sync + 'static {
    fn event_name(&self) -> &'static str;
}

/// Placeholder event handler trait
pub trait EventHandler: Send + Sync + 'static {
    type E: Event;
}

/// Placeholder event bus
pub struct EventBus;

impl EventBus {
    pub fn new() -> Self {
        Self
    }

    pub async fn register_handler<H: EventHandler>(&self, _handler: H) {
        // Placeholder
    }

    pub async fn publish<E: Event>(&self, _event: E) -> crate::KernelResult<()> {
        Ok(())
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

// Placeholder system events

/// Kernel started event
#[derive(Debug)]
pub struct KernelStartedEvent;

impl Event for KernelStartedEvent {
    fn event_name(&self) -> &'static str {
        "kernel.started"
    }
}

/// Kernel stopped event
#[derive(Debug)]
pub struct KernelStoppedEvent;

impl Event for KernelStoppedEvent {
    fn event_name(&self) -> &'static str {
        "kernel.stopped"
    }
}

/// Plugin loaded event
#[derive(Debug)]
#[allow(dead_code)]
pub struct PluginLoadedEvent {
    pub plugin_name: String,
    pub plugin_version: String,
}

impl Event for PluginLoadedEvent {
    fn event_name(&self) -> &'static str {
        "plugin.loaded"
    }
}
