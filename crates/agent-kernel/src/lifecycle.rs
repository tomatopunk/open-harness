use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 生命周期钩子类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleStage {
    /// 初始化之前
    BeforeInit,
    /// 初始化配置阶段
    InitConfig,
    /// 初始化提供者阶段
    InitProvider,
    /// 初始化 MCP 阶段
    InitMcp,
    /// 初始化插件阶段
    InitPlugin,
    /// 初始化循环/状态阶段
    InitLoopState,
    /// 初始化阶段
    Init,
    /// 初始化之后
    AfterInit,
    /// 启动之前
    BeforeStart,
    /// 启动阶段
    Start,
    /// 启动之后
    AfterStart,
    /// 停止之前
    BeforeStop,
    /// 停止阶段
    Stop,
    /// 停止之后
    AfterStop,
    /// 清理阶段
    Cleanup,
}

/// 生命周期钩子 trait
#[async_trait]
pub trait LifecycleHook: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    async fn on_lifecycle(&self, stage: LifecycleStage) -> crate::KernelResult<()>;
}

/// 生命周期管理器
pub struct LifecycleManager {
    hooks: RwLock<HashMap<LifecycleStage, Vec<Arc<dyn LifecycleHook>>>>,
}

impl LifecycleManager {
    pub fn new() -> Self {
        Self { hooks: RwLock::new(HashMap::new()) }
    }

    /// 注册生命周期钩子
    pub async fn register_hook(&self, stage: LifecycleStage, hook: Arc<dyn LifecycleHook>) {
        let mut hooks = self.hooks.write().await;
        hooks.entry(stage).or_insert_with(Vec::new).push(hook);
    }

    /// 触发生命周期阶段
    pub async fn trigger_stage(&self, stage: LifecycleStage) -> crate::KernelResult<()> {
        tracing::debug!("Triggering lifecycle stage: {:?}", stage);

        let hooks = self.hooks.read().await;
        if let Some(hooks) = hooks.get(&stage) {
            for hook in hooks {
                tracing::debug!("Executing lifecycle hook: {}", hook.name());
                hook.on_lifecycle(stage).await?;
            }
        }

        Ok(())
    }

    /// 按顺序运行多个阶段
    pub async fn run_stages(&self, stages: &[LifecycleStage]) -> crate::KernelResult<()> {
        for &stage in stages {
            self.trigger_stage(stage).await?;
        }
        Ok(())
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 获取标准启动阶段序列
#[allow(dead_code)]
pub fn startup_stages() -> Vec<LifecycleStage> {
    vec![
        LifecycleStage::BeforeInit,
        LifecycleStage::Init,
        LifecycleStage::AfterInit,
        LifecycleStage::BeforeStart,
        LifecycleStage::Start,
        LifecycleStage::AfterStart,
    ]
}

/// 获取标准停止阶段序列
pub fn shutdown_stages() -> Vec<LifecycleStage> {
    vec![
        LifecycleStage::BeforeStop,
        LifecycleStage::Stop,
        LifecycleStage::AfterStop,
        LifecycleStage::Cleanup,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct TestHook {
        log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl LifecycleHook for TestHook {
        fn name(&self) -> &'static str {
            "TestHook"
        }

        async fn on_lifecycle(&self, stage: LifecycleStage) -> crate::KernelResult<()> {
            let mut v = self.log.lock().await;
            v.push(format!("{:?}", stage));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_explicit_stages_order_invoked() {
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let hook = Arc::new(TestHook { log: log.clone() });
        let lifecycle = LifecycleManager::new();

        lifecycle.register_hook(LifecycleStage::InitConfig, hook.clone()).await;
        lifecycle.register_hook(LifecycleStage::InitProvider, hook.clone()).await;
        lifecycle.register_hook(LifecycleStage::InitMcp, hook.clone()).await;
        lifecycle.register_hook(LifecycleStage::InitPlugin, hook.clone()).await;
        lifecycle.register_hook(LifecycleStage::InitLoopState, hook.clone()).await;
        lifecycle.register_hook(LifecycleStage::Init, hook.clone()).await;
        lifecycle.register_hook(LifecycleStage::AfterInit, hook.clone()).await;

        lifecycle
            .run_stages(&[
                LifecycleStage::InitConfig,
                LifecycleStage::InitProvider,
                LifecycleStage::InitMcp,
                LifecycleStage::InitPlugin,
                LifecycleStage::InitLoopState,
                LifecycleStage::Init,
                LifecycleStage::AfterInit,
            ])
            .await
            .unwrap();

        let captured = log.lock().await.clone();
        let expected = vec![
            "InitConfig",
            "InitProvider",
            "InitMcp",
            "InitPlugin",
            "InitLoopState",
            "Init",
            "AfterInit",
        ];
        assert_eq!(captured, expected);
    }
}
