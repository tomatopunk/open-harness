//! Channel manager for IM integration.
//!
//! Provides abstraction for different IM channels (Dingtalk, Wecom, etc.)
//! and manages their lifecycle based on configuration.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::config::ChannelsConfig;

/// Inbound message received from external channel.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// Channel name this came from
    pub channel: String,
    /// User ID
    pub user_id: String,
    /// Message content
    pub content: String,
    /// Thread ID (if existing conversation)
    pub thread_id: Option<String>,
    /// Event ID for deduplication
    pub event_id: Option<String>,
}

/// Outbound message to be sent to external channel.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    /// User ID to send to
    pub user_id: String,
    /// Message content
    pub content: String,
    /// Thread ID
    pub thread_id: Option<String>,
}

/// Channel trait that all IM channels must implement.
#[async_trait]
pub trait Channel: Send + Sync + 'static {
    /// Get the channel name
    fn name(&self) -> &str;

    /// Start the channel (listen for incoming messages)
    async fn start(&self) -> Result<()>;

    /// Stop the channel
    async fn stop(&self) -> Result<()>;

    /// Send an outbound message
    async fn send(&self, message: OutboundMessage) -> Result<()>;
}

/// Channel manager manages all configured channels.
pub struct ChannelManager {
    channels: RwLock<HashMap<String, Box<dyn Channel>>>,
    config: ChannelsConfig,
}

impl ChannelManager {
    /// Create a new channel manager from configuration.
    pub fn new(config: ChannelsConfig) -> Self {
        Self { channels: RwLock::new(HashMap::new()), config }
    }

    /// Register a channel.
    pub fn register_channel(&self, channel: Box<dyn Channel>) {
        let mut channels =
            tokio::runtime::Handle::current().block_on(async { self.channels.write().await });
        channels.insert(channel.name().to_string(), channel);
    }

    /// Start all enabled channels.
    pub async fn start_all(&self) -> Result<()> {
        let channels = self.channels.read().await;
        for (_, channel) in channels.iter() {
            channel.start().await?;
        }
        Ok(())
    }

    /// Stop all channels.
    pub async fn stop_all(&self) -> Result<()> {
        let channels = self.channels.read().await;
        for (_, channel) in channels.iter() {
            channel.stop().await?;
        }
        Ok(())
    }

    /// Check if a channel exists by name.
    pub async fn has_channel(&self, name: &str) -> bool {
        let channels = self.channels.read().await;
        channels.contains_key(name)
    }

    /// Send a message to a specific channel.
    pub async fn send_to(&self, channel_name: &str, message: OutboundMessage) -> Result<()> {
        let channels = self.channels.read().await;
        if let Some(channel) = channels.get(channel_name) {
            channel.send(message).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Channel not found: {}", channel_name))
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &ChannelsConfig {
        &self.config
    }
}
