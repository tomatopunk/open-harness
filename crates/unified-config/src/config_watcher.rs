//! Configuration hot-reload and synchronization using watch channels.
//!
//! This module provides a mechanism for notifying components when the unified configuration
//! changes, allowing them to react to configuration updates in real-time.

use crate::UnifiedConfig;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

/// Configuration watcher for hot-reload support.
///
/// Uses tokio's watch channel to broadcast configuration changes to all subscribers.
#[derive(Clone)]
pub struct ConfigWatcher {
    sender: Arc<watch::Sender<UnifiedConfig>>,
}

impl ConfigWatcher {
    /// Create a new ConfigWatcher with the initial configuration.
    pub fn new(config: UnifiedConfig) -> Self {
        let (sender, _receiver) = watch::channel(config);
        Self { sender: Arc::new(sender) }
    }

    /// Update the configuration and notify all subscribers.
    pub fn update(
        &self,
        new_config: UnifiedConfig,
    ) -> Result<(), Box<watch::error::SendError<UnifiedConfig>>> {
        self.sender.send(new_config).map_err(Box::new)
    }

    /// Subscribe to configuration changes.
    ///
    /// Returns a receiver that can be used to watch for configuration updates.
    pub fn subscribe(&self) -> watch::Receiver<UnifiedConfig> {
        self.sender.subscribe()
    }

    /// Get the current configuration without waiting for changes.
    pub fn get_current(&self) -> UnifiedConfig {
        self.sender.borrow().clone()
    }

    /// Check if there are any active subscribers.
    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// Configuration manager that combines watch-based notifications with RwLock-based access.
///
/// This is the recommended way to manage unified configuration in applications.
#[derive(Clone)]
pub struct ConfigManager {
    watcher: ConfigWatcher,
    /// The underlying configuration storage. Public for integration with existing code.
    pub config: Arc<RwLock<UnifiedConfig>>,
}

impl ConfigManager {
    /// Create a new ConfigManager with the initial configuration.
    pub fn new(config: UnifiedConfig) -> Self {
        let watcher = ConfigWatcher::new(config.clone());
        let config = Arc::new(RwLock::new(config));
        Self { watcher, config }
    }

    /// Get a reference to the underlying watcher.
    pub fn watcher(&self) -> &ConfigWatcher {
        &self.watcher
    }

    /// Read the current configuration.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, UnifiedConfig> {
        self.config.read().await
    }

    /// Write the configuration and notify subscribers.
    pub async fn write(&self, new_config: UnifiedConfig) {
        // Update the RwLock
        {
            let mut config = self.config.write().await;
            *config = new_config.clone();
        }

        // Notify subscribers (this also updates the watcher's internal state)
        let _ = self.watcher.update(new_config);
    }

    /// Reload configuration from a loader function and notify subscribers.
    pub async fn reload<F, E>(&self, loader: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<UnifiedConfig, E>,
    {
        let new_config = loader()?;
        self.write(new_config).await;
        Ok(())
    }

    /// Subscribe to configuration changes.
    pub fn subscribe(&self) -> watch::Receiver<UnifiedConfig> {
        self.watcher.subscribe()
    }

    /// Get the current configuration without waiting for changes.
    pub fn get_current(&self) -> UnifiedConfig {
        // Use the RwLock's blocking read since this is a synchronous method
        // In async context, prefer using read().await
        self.config.try_read().map(|c| c.clone()).unwrap_or_else(|_e| self.watcher.get_current())
    }
}

// Note: create_config_manager_from_app_config has been moved to config-runtime
// to avoid circular dependencies.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ModelConfig, ModelEntry, ModelRegistry, PolicySwitches, SubagentConfig, ToolRegistry,
    };

    fn create_test_config() -> UnifiedConfig {
        UnifiedConfig {
            models: ModelRegistry {
                default_model: "gpt-4".to_string(),
                entries: vec![ModelEntry {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    provider: "langchain_openai:ChatOpenAI".to_string(),
                    model_id: "gpt-4".to_string(),
                    config: ModelConfig::default(),
                }],
            },
            tools: ToolRegistry::default(),
            subagents: SubagentConfig::default(),
            policies: PolicySwitches::default(),
            acp_agents: crate::ACPAgentsConfig::default(),
        }
    }

    #[tokio::test]
    async fn test_config_watcher_update_and_subscribe() {
        let initial_config = create_test_config();
        let watcher = ConfigWatcher::new(initial_config);

        // Subscribe before update
        let mut rx = watcher.subscribe();

        // Update configuration
        let mut new_config = create_test_config();
        new_config.models.default_model = "gpt-5".to_string();
        watcher.update(new_config.clone()).unwrap();

        // Check that receiver gets the update
        rx.changed().await.unwrap();
        // Manual comparison since UnifiedConfig doesn't implement PartialEq
        let received = rx.borrow();
        assert_eq!(received.models.default_model, new_config.models.default_model);
        assert_eq!(received.models.entries.len(), new_config.models.entries.len());
    }

    #[tokio::test]
    async fn test_config_manager_write_and_read() {
        let initial_config = create_test_config();
        let manager = ConfigManager::new(initial_config);

        // Read initial config
        {
            let config = manager.read().await;
            assert_eq!(config.models.default_model, "gpt-4");
        }

        // Write new config
        let mut new_config = create_test_config();
        new_config.models.default_model = "gpt-4-turbo".to_string();
        manager.write(new_config.clone()).await;

        // Read updated config immediately (write is synchronous within the lock)
        {
            let config = manager.read().await;
            assert_eq!(config.models.default_model, "gpt-4-turbo");
        }

        // Check current config without lock
        let current = manager.get_current();
        assert_eq!(current.models.default_model, "gpt-4-turbo");
    }

    #[tokio::test]
    async fn test_config_manager_reload() {
        let initial_config = create_test_config();
        let manager = ConfigManager::new(initial_config);

        // Subscribe to watch for updates
        let mut rx = manager.subscribe();

        // Reload with new config
        manager
            .reload(|| -> Result<UnifiedConfig, &'static str> {
                let mut config = create_test_config();
                config.policies.max_turns = 32;
                Ok(config)
            })
            .await
            .unwrap();

        // Check that receiver gets the update
        rx.changed().await.unwrap();
        let config = manager.read().await;
        assert_eq!(config.policies.max_turns, 32);
    }

    #[test]
    fn test_subscriber_count() {
        let config = create_test_config();
        let watcher = ConfigWatcher::new(config);

        assert_eq!(watcher.subscriber_count(), 0);
        assert!(!watcher.has_subscribers());

        let _rx1 = watcher.subscribe();
        assert_eq!(watcher.subscriber_count(), 1);
        assert!(watcher.has_subscribers());

        let _rx2 = watcher.subscribe();
        assert_eq!(watcher.subscriber_count(), 2);
    }
}
