use crate::{Plugin, PluginContext, PluginError, PluginManifest, PluginResult};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// 插件管理器
pub struct PluginManager {
    plugins_dir: PathBuf,
    discovered: RwLock<HashMap<String, PluginManifest>>,
    loaded: RwLock<HashMap<String, Box<dyn Plugin>>>,
    context: PluginContext,
}

impl PluginManager {
    /// 创建新的插件管理器
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            context: PluginContext::new(
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ),
            plugins_dir,
            discovered: RwLock::new(HashMap::new()),
            loaded: RwLock::new(HashMap::new()),
        }
    }

    /// 发现插件
    pub async fn discover_plugins(&self) -> PluginResult<Vec<PluginManifest>> {
        info!("Discovering plugins in: {:?}", self.plugins_dir);

        let mut discovered = self.discovered.write().await;
        discovered.clear();

        if !self.plugins_dir.exists() {
            debug!("Plugins directory does not exist, skipping discovery");
            return Ok(vec![]);
        }

        let mut manifests = Vec::new();

        // 扫描插件目录
        let mut entries = tokio::fs::read_dir(&self.plugins_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                let manifest_path = path.join("plugin.yaml");
                if manifest_path.exists() {
                    match PluginManifest::from_file(&manifest_path) {
                        Ok(manifest) => {
                            if manifest.enabled {
                                info!("Discovered plugin: {} v{}", manifest.name, manifest.version);
                                discovered.insert(manifest.name.clone(), manifest.clone());
                                manifests.push(manifest);
                            } else {
                                debug!("Plugin {} is disabled, skipping", manifest.name);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to load manifest from {:?}: {}", manifest_path, e);
                        }
                    }
                }
            }
        }

        Ok(manifests)
    }

    /// 加载单个插件
    pub async fn load_plugin(&self, name: &str) -> PluginResult<()> {
        let discovered = self.discovered.read().await;
        let manifest =
            discovered.get(name).ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        info!("Loading plugin: {} v{}", manifest.name, manifest.version);

        // 检查是否已加载
        {
            let loaded = self.loaded.read().await;
            if loaded.contains_key(name) {
                return Err(PluginError::AlreadyLoaded(name.to_string()));
            }
        }

        // 这里简化处理：创建基础插件实例
        // 在实际实现中，这里应该支持动态加载
        let plugin = Box::new(crate::plugin::BasePlugin::new(manifest.clone()));

        // 加载插件
        let mut plugin = plugin;
        plugin.load(&self.context).await?;

        let mut loaded = self.loaded.write().await;
        loaded.insert(name.to_string(), plugin);

        Ok(())
    }

    /// 加载所有插件
    pub async fn load_all_plugins(&self) -> PluginResult<()> {
        let names: Vec<String> = {
            let discovered = self.discovered.read().await;
            discovered.keys().cloned().collect()
        };

        for name in names {
            if let Err(e) = self.load_plugin(&name).await {
                warn!("Failed to load plugin {}: {}", name, e);
            }
        }

        Ok(())
    }

    /// 初始化插件
    pub async fn initialize_plugin(&self, name: &str) -> PluginResult<()> {
        let mut loaded = self.loaded.write().await;
        let plugin =
            loaded.get_mut(name).ok_or_else(|| PluginError::NotLoaded(name.to_string()))?;

        info!("Initializing plugin: {}", name);
        plugin.initialize(&self.context).await?;

        Ok(())
    }

    /// 启动插件
    pub async fn start_plugin(&self, name: &str) -> PluginResult<()> {
        let mut loaded = self.loaded.write().await;
        let plugin =
            loaded.get_mut(name).ok_or_else(|| PluginError::NotLoaded(name.to_string()))?;

        info!("Starting plugin: {}", name);
        plugin.start(&self.context).await?;

        Ok(())
    }

    /// 停止插件
    pub async fn stop_plugin(&self, name: &str) -> PluginResult<()> {
        let mut loaded = self.loaded.write().await;
        let plugin =
            loaded.get_mut(name).ok_or_else(|| PluginError::NotLoaded(name.to_string()))?;

        info!("Stopping plugin: {}", name);
        plugin.stop(&self.context).await?;

        Ok(())
    }

    /// 卸载插件
    pub async fn unload_plugin(&self, name: &str) -> PluginResult<()> {
        let mut loaded = self.loaded.write().await;
        let mut plugin =
            loaded.remove(name).ok_or_else(|| PluginError::NotLoaded(name.to_string()))?;

        info!("Unloading plugin: {}", name);
        plugin.unload(&self.context).await?;

        Ok(())
    }

    /// 卸载所有插件
    pub async fn unload_all_plugins(&self) -> PluginResult<()> {
        let names: Vec<String> = {
            let loaded = self.loaded.read().await;
            loaded.keys().cloned().collect()
        };

        for name in names {
            if let Err(e) = self.unload_plugin(&name).await {
                warn!("Failed to unload plugin {}: {}", name, e);
            }
        }

        Ok(())
    }

    /// 获取已发现插件列表
    pub async fn discovered_plugins(&self) -> Vec<PluginManifest> {
        let discovered = self.discovered.read().await;
        discovered.values().cloned().collect()
    }

    /// 获取已加载插件列表
    pub async fn loaded_plugins(&self) -> Vec<String> {
        let loaded = self.loaded.read().await;
        loaded.keys().cloned().collect()
    }
}
