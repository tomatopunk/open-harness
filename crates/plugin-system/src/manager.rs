use crate::{
    BasePlugin, Plugin, PluginContext, PluginError, PluginLifecycleStage, PluginManifest,
    PluginResult, PluginState,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

type PluginFactory = Arc<dyn Fn(PluginManifest) -> Box<dyn Plugin> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    pub name: String,
    pub state: PluginState,
    pub last_stage: PluginLifecycleStage,
}

pub struct PluginManager {
    plugins_dir: PathBuf,
    discovered: RwLock<HashMap<String, PluginManifest>>,
    loaded: RwLock<HashMap<String, Box<dyn Plugin>>>,
    factories: RwLock<HashMap<String, PluginFactory>>,
    plugin_configs: RwLock<HashMap<String, Value>>,
    statuses: RwLock<HashMap<String, PluginStatus>>,
    context: PluginContext,
}

impl PluginManager {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self::new_with_workspace_dir(
            plugins_dir,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    }

    pub fn new_with_workspace_dir(plugins_dir: PathBuf, workspace_dir: PathBuf) -> Self {
        Self {
            context: PluginContext::new(workspace_dir),
            plugins_dir,
            discovered: RwLock::new(HashMap::new()),
            loaded: RwLock::new(HashMap::new()),
            factories: RwLock::new(HashMap::new()),
            plugin_configs: RwLock::new(HashMap::new()),
            statuses: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register_factory<F>(&self, name: impl Into<String>, factory: F)
    where
        F: Fn(PluginManifest) -> Box<dyn Plugin> + Send + Sync + 'static,
    {
        self.factories.write().await.insert(name.into(), Arc::new(factory));
    }

    pub async fn set_plugin_config(&self, name: impl Into<String>, config: Value) {
        self.plugin_configs.write().await.insert(name.into(), config);
    }

    pub async fn discover_plugins(&self) -> PluginResult<Vec<PluginManifest>> {
        info!("Discovering plugins in: {:?}", self.plugins_dir);

        let mut discovered = self.discovered.write().await;
        let mut statuses = self.statuses.write().await;
        discovered.clear();
        statuses.clear();

        if !self.plugins_dir.exists() {
            debug!("Plugins directory does not exist, skipping discovery");
            return Ok(vec![]);
        }

        let mut manifests = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.plugins_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("plugin.yaml");
            if !manifest_path.exists() {
                continue;
            }

            match PluginManifest::from_file(&manifest_path) {
                Ok(manifest) => {
                    if manifest.enabled {
                        info!("Discovered plugin: {} v{}", manifest.name, manifest.version);
                        statuses.insert(
                            manifest.name.clone(),
                            PluginStatus {
                                name: manifest.name.clone(),
                                state: PluginState::Discovered,
                                last_stage: PluginLifecycleStage::Discovery,
                            },
                        );
                        discovered.insert(manifest.name.clone(), manifest.clone());
                        manifests.push(manifest);
                    } else {
                        debug!("Plugin {} is disabled, skipping", manifest.name);
                    }
                }
                Err(error) => {
                    warn!("Failed to load manifest from {:?}: {}", manifest_path, error);
                }
            }
        }

        Ok(manifests)
    }

    pub async fn load_plugin(&self, name: &str) -> PluginResult<()> {
        let manifest = {
            let discovered = self.discovered.read().await;
            discovered.get(name).cloned().ok_or_else(|| PluginError::NotFound(name.to_string()))?
        };

        {
            let loaded = self.loaded.read().await;
            if loaded.contains_key(name) {
                return Err(PluginError::AlreadyLoaded(name.to_string()));
            }
        }

        info!("Loading plugin: {} v{}", manifest.name, manifest.version);

        let factory = {
            let factories = self.factories.read().await;
            factories.get(name).cloned()
        };

        let mut plugin = match factory {
            Some(factory) => factory(manifest),
            None => Box::new(BasePlugin::new(manifest)),
        };

        let context = self.context_for(name).await;
        plugin
            .load(&context)
            .await
            .map_err(|error| self.lifecycle_error(name, PluginLifecycleStage::Load, error))?;

        self.set_status(name, PluginState::Loaded, PluginLifecycleStage::Load).await;
        self.loaded.write().await.insert(name.to_string(), plugin);

        Ok(())
    }

    pub async fn load_all_plugins(&self) -> PluginResult<()> {
        for name in self.discovered_names().await {
            self.load_plugin(&name).await?;
        }

        Ok(())
    }

    pub async fn initialize_plugin(&self, name: &str) -> PluginResult<()> {
        self.run_loaded_stage(name, PluginLifecycleStage::Initialize).await
    }

    pub async fn initialize_all_plugins(&self) -> PluginResult<()> {
        for name in self.loaded_plugin_names().await {
            self.initialize_plugin(&name).await?;
        }

        Ok(())
    }

    pub async fn start_plugin(&self, name: &str) -> PluginResult<()> {
        self.run_loaded_stage(name, PluginLifecycleStage::Start).await
    }

    pub async fn start_all_plugins(&self) -> PluginResult<()> {
        for name in self.loaded_plugin_names().await {
            self.start_plugin(&name).await?;
        }

        Ok(())
    }

    pub async fn stop_plugin(&self, name: &str) -> PluginResult<()> {
        self.run_loaded_stage(name, PluginLifecycleStage::Stop).await
    }

    pub async fn stop_all_plugins(&self) -> PluginResult<()> {
        let mut names = self.loaded_plugin_names().await;
        names.reverse();

        for name in names {
            self.stop_plugin(&name).await?;
        }

        Ok(())
    }

    pub async fn unload_plugin(&self, name: &str) -> PluginResult<()> {
        let mut plugin = self
            .loaded
            .write()
            .await
            .remove(name)
            .ok_or_else(|| PluginError::NotLoaded(name.to_string()))?;

        info!("Unloading plugin: {}", name);

        let context = self.context_for(name).await;
        plugin
            .unload(&context)
            .await
            .map_err(|error| self.lifecycle_error(name, PluginLifecycleStage::Unload, error))?;

        self.set_status(name, PluginState::Unloaded, PluginLifecycleStage::Unload).await;

        Ok(())
    }

    pub async fn unload_all_plugins(&self) -> PluginResult<()> {
        let mut names = self.loaded_plugin_names().await;
        names.reverse();

        for name in names {
            self.unload_plugin(&name).await?;
        }

        Ok(())
    }

    pub async fn discovered_plugins(&self) -> Vec<PluginManifest> {
        self.discovered.read().await.values().cloned().collect()
    }

    pub async fn loaded_plugins(&self) -> Vec<String> {
        self.loaded.read().await.keys().cloned().collect()
    }

    pub async fn plugin_status(&self, name: &str) -> Option<PluginStatus> {
        self.statuses.read().await.get(name).cloned()
    }

    pub async fn plugin_statuses(&self) -> Vec<PluginStatus> {
        self.statuses.read().await.values().cloned().collect()
    }

    async fn discovered_names(&self) -> Vec<String> {
        self.discovered.read().await.keys().cloned().collect()
    }

    async fn loaded_plugin_names(&self) -> Vec<String> {
        self.loaded.read().await.keys().cloned().collect()
    }

    async fn context_for(&self, name: &str) -> PluginContext {
        let config = self.plugin_configs.read().await.get(name).cloned();
        self.context.with_config(config)
    }

    async fn set_status(&self, name: &str, state: PluginState, last_stage: PluginLifecycleStage) {
        self.statuses
            .write()
            .await
            .insert(name.to_string(), PluginStatus { name: name.to_string(), state, last_stage });
    }

    async fn run_loaded_stage(&self, name: &str, stage: PluginLifecycleStage) -> PluginResult<()> {
        let mut loaded = self.loaded.write().await;
        let plugin =
            loaded.get_mut(name).ok_or_else(|| PluginError::NotLoaded(name.to_string()))?;

        info!("{} plugin: {}", stage, name);

        let context = self.context_for(name).await;
        let state = match stage {
            PluginLifecycleStage::Initialize => {
                plugin.initialize(&context).await.map(|_| PluginState::Initialized)
            }
            PluginLifecycleStage::Start => {
                plugin.start(&context).await.map(|_| PluginState::Running)
            }
            PluginLifecycleStage::Stop => plugin.stop(&context).await.map(|_| PluginState::Stopped),
            PluginLifecycleStage::Discovery
            | PluginLifecycleStage::Load
            | PluginLifecycleStage::Unload => unreachable!(),
        }
        .map_err(|error| self.lifecycle_error(name, stage, error))?;

        drop(loaded);
        self.set_status(name, state, stage).await;

        Ok(())
    }

    fn lifecycle_error(
        &self,
        name: &str,
        stage: PluginLifecycleStage,
        error: PluginError,
    ) -> PluginError {
        PluginError::LifecycleFailed { plugin: name.to_string(), stage, source: Box::new(error) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde::Deserialize;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestPlugin {
        base: BasePlugin,
        fail_initialize: bool,
    }

    #[derive(Deserialize)]
    struct TestPluginConfig {
        bind: String,
    }

    impl TestPlugin {
        fn new(manifest: PluginManifest, fail_initialize: bool) -> Self {
            Self { base: BasePlugin::new(manifest), fail_initialize }
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn manifest(&self) -> &PluginManifest {
            self.base.manifest()
        }

        fn state(&self) -> PluginState {
            self.base.state()
        }

        async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
            if let Some(config) = &ctx.config {
                let parsed = serde_json::from_value::<TestPluginConfig>(config.clone()).map_err(
                    |error| PluginError::InvalidConfiguration {
                        plugin: self.manifest().name.clone(),
                        details: error.to_string(),
                    },
                )?;
                let _ = parsed.bind;
            }

            self.base.load(ctx).await
        }

        async fn initialize(&mut self, ctx: &PluginContext) -> PluginResult<()> {
            if self.fail_initialize {
                return Err(PluginError::InitializationFailed("boom".to_string()));
            }

            self.base.initialize(ctx).await
        }

        async fn start(&mut self, ctx: &PluginContext) -> PluginResult<()> {
            self.base.start(ctx).await
        }

        async fn stop(&mut self, ctx: &PluginContext) -> PluginResult<()> {
            self.base.stop(ctx).await
        }

        async fn unload(&mut self, ctx: &PluginContext) -> PluginResult<()> {
            self.base.unload(ctx).await
        }
    }

    #[tokio::test]
    async fn plugin_manager_tracks_explicit_lifecycle_stages() {
        let plugins_dir = create_plugin_dir("test-plugin");
        let manager = PluginManager::new(plugins_dir.clone());

        manager
            .register_factory("test-plugin", |manifest| Box::new(TestPlugin::new(manifest, false)))
            .await;

        manager.discover_plugins().await.unwrap();
        assert_status(
            &manager,
            "test-plugin",
            PluginState::Discovered,
            PluginLifecycleStage::Discovery,
        )
        .await;

        manager.load_all_plugins().await.unwrap();
        assert_status(&manager, "test-plugin", PluginState::Loaded, PluginLifecycleStage::Load)
            .await;

        manager.initialize_all_plugins().await.unwrap();
        assert_status(
            &manager,
            "test-plugin",
            PluginState::Initialized,
            PluginLifecycleStage::Initialize,
        )
        .await;

        manager.start_all_plugins().await.unwrap();
        assert_status(&manager, "test-plugin", PluginState::Running, PluginLifecycleStage::Start)
            .await;

        manager.stop_all_plugins().await.unwrap();
        assert_status(&manager, "test-plugin", PluginState::Stopped, PluginLifecycleStage::Stop)
            .await;

        manager.unload_all_plugins().await.unwrap();
        assert_status(&manager, "test-plugin", PluginState::Unloaded, PluginLifecycleStage::Unload)
            .await;

        fs::remove_dir_all(plugins_dir).unwrap();
    }

    #[tokio::test]
    async fn plugin_manager_reports_plugin_specific_load_failures() {
        let plugins_dir = create_plugin_dir("broken-config-plugin");
        let manager = PluginManager::new(plugins_dir.clone());

        manager
            .register_factory("broken-config-plugin", |manifest| {
                Box::new(TestPlugin::new(manifest, false))
            })
            .await;
        manager
            .set_plugin_config(
                "broken-config-plugin",
                serde_json::json!({
                    "bind": 8080,
                }),
            )
            .await;

        manager.discover_plugins().await.unwrap();
        let error = manager.load_all_plugins().await.unwrap_err();

        let rendered = error.to_string();
        assert!(rendered.contains("broken-config-plugin"));

        match error {
            PluginError::LifecycleFailed { plugin, stage, source } => {
                assert_eq!(plugin, "broken-config-plugin");
                assert_eq!(stage, PluginLifecycleStage::Load);
                assert_eq!(source.category(), crate::PluginErrorCategory::Config);
                match source.as_ref() {
                    PluginError::InvalidConfiguration { plugin, details } => {
                        assert_eq!(plugin, "broken-config-plugin");
                        assert!(details.contains("invalid type"));
                    }
                    other => panic!("unexpected nested error: {other:?}"),
                }
            }
            other => panic!("unexpected error: {other:?}"),
        }

        assert_status(
            &manager,
            "broken-config-plugin",
            PluginState::Discovered,
            PluginLifecycleStage::Discovery,
        )
        .await;

        fs::remove_dir_all(plugins_dir).unwrap();
    }

    #[tokio::test]
    async fn plugin_manager_reports_plugin_specific_initialize_failures() {
        let plugins_dir = create_plugin_dir("failing-plugin");
        let manager = PluginManager::new(plugins_dir.clone());

        manager
            .register_factory("failing-plugin", |manifest| {
                Box::new(TestPlugin::new(manifest, true))
            })
            .await;

        manager.discover_plugins().await.unwrap();
        manager.load_all_plugins().await.unwrap();

        let error = manager.initialize_all_plugins().await.unwrap_err();

        let rendered = error.to_string();
        assert!(rendered.contains("failing-plugin"));

        match error {
            PluginError::LifecycleFailed { plugin, stage, source } => {
                assert_eq!(plugin, "failing-plugin");
                assert_eq!(stage, PluginLifecycleStage::Initialize);
                assert_eq!(source.category(), crate::PluginErrorCategory::Initialization);
                match source.as_ref() {
                    PluginError::InitializationFailed(details) => {
                        assert!(details.contains("boom"));
                    }
                    other => panic!("unexpected nested error: {other:?}"),
                }
            }
            other => panic!("unexpected error: {other:?}"),
        }

        assert_status(&manager, "failing-plugin", PluginState::Loaded, PluginLifecycleStage::Load)
            .await;

        fs::remove_dir_all(plugins_dir).unwrap();
    }

    async fn assert_status(
        manager: &PluginManager,
        name: &str,
        expected_state: PluginState,
        expected_stage: PluginLifecycleStage,
    ) {
        let status = manager.plugin_status(name).await.expect("plugin status should exist");
        assert_eq!(status.state, expected_state);
        assert_eq!(status.last_stage, expected_stage);
    }

    fn create_plugin_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("plugin-system-tests-{name}-{now}-{suffix}"));
        let plugin_dir = root.join(name);

        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.yaml"),
            format!(
                "name: {name}\nversion: 0.1.0\ndescription: test plugin\nauthors:\n  - test\ntype: generic\nenabled: true\ndependencies: []\n"
            ),
        )
        .unwrap();

        root
    }
}
