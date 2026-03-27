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
                let driver = DingTalkDriver {
                    secret: std::env::var("DINGTALK_SECRET").unwrap_or_else(|_| "dev".to_string()),
                };
                registry.register(Box::new(driver));
            }
            WECOM => {
                registry.register(Box::new(WeComDriver));
            }
            _ => {}
        }
    }
    registry.list_platforms()
}

pub fn configured_channels(config: &AppConfig) -> Vec<String> {
    if config.channels.enabled.is_empty() {
        return builtin_channels();
    }
    config
        .channels
        .enabled
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| matches!(name.as_str(), DINGTALK | WECOM))
        .collect()
}

pub fn builtin_channels() -> Vec<String> {
    vec![DINGTALK.to_string(), WECOM.to_string()]
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
