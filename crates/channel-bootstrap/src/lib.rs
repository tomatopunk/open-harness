use channel_dingtalk::DingTalkDriver;
use channel_runtime::ChannelRegistry;
use channel_wecom::WeComDriver;
use config_runtime::AppConfig;

const DINGTALK: &str = "dingtalk";
const WECOM: &str = "wecom";

pub fn register_builtin_channels(
    registry: &mut ChannelRegistry,
    config: &AppConfig,
) -> Vec<String> {
    let enabled = configured_channels(config);
    for platform in &enabled {
        match platform.as_str() {
            DINGTALK => {
                if let Some(secret) = resolve_dingtalk_secret() {
                    let driver = DingTalkDriver {
                        secret,
                        webhook_url: std::env::var("DINGTALK_WEBHOOK_URL").ok(),
                        client: reqwest::Client::new(),
                    };
                    registry.register(Box::new(driver));
                }
            }
            WECOM => {
                let secret = std::env::var("WECOM_SECRET").ok();
                registry.register(Box::new(WeComDriver {
                    secret,
                    webhook_url: std::env::var("WECOM_WEBHOOK_URL").ok(),
                    client: reqwest::Client::new(),
                }));
            }
            _ => {
                tracing::warn!(platform = %platform, "unsupported channel platform configured");
            }
        }
    }
    registry.list_platforms()
}

pub fn configured_channels(config: &AppConfig) -> Vec<String> {
    if config.channels.enabled.is_empty() {
        return builtin_channels();
    }
    let mut result = Vec::new();
    for name in &config.channels.enabled {
        let normalized = name.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), DINGTALK | WECOM) {
            result.push(normalized);
        } else {
            tracing::warn!(platform = %normalized, "unknown channel ignored");
        }
    }
    result
}

pub fn builtin_channels() -> Vec<String> {
    vec![DINGTALK.to_string(), WECOM.to_string()]
}

fn resolve_dingtalk_secret() -> Option<String> {
    if let Ok(secret) = std::env::var("DINGTALK_SECRET") {
        return Some(secret);
    }
    if is_production() {
        tracing::error!("DINGTALK_SECRET is required in production; dingtalk disabled");
        return None;
    }
    tracing::warn!("DINGTALK_SECRET missing; using development fallback secret");
    Some("dev".to_string())
}

fn is_production() -> bool {
    let env = std::env::var("OPEN_HARNESS_ENV").unwrap_or_default().to_ascii_lowercase();
    matches!(env.as_str(), "prod" | "production")
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_runtime::{AppConfig, ChannelsConfig};

    #[test]
    fn configured_channels_uses_defaults_when_empty() {
        let config =
            AppConfig { channels: ChannelsConfig { enabled: Vec::new() }, ..AppConfig::default() };
        assert_eq!(configured_channels(&config), vec!["dingtalk".to_string(), "wecom".to_string()]);
    }

    #[test]
    fn configured_channels_filters_unknown_values() {
        let config = AppConfig {
            channels: ChannelsConfig {
                enabled: vec!["wecom".to_string(), "unknown".to_string(), "DINGTALK".to_string()],
            },
            ..AppConfig::default()
        };
        assert_eq!(configured_channels(&config), vec!["wecom".to_string(), "dingtalk".to_string()]);
    }

    #[test]
    fn register_builtin_channels_registers_enabled_drivers() {
        let config = AppConfig {
            channels: ChannelsConfig { enabled: vec!["wecom".to_string()] },
            ..AppConfig::default()
        };
        let mut registry = ChannelRegistry::new();
        let registered = register_builtin_channels(&mut registry, &config);
        assert_eq!(registered, vec!["wecom".to_string()]);
    }
}
