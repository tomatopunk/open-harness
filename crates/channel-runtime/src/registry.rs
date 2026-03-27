use std::collections::HashMap;

use crate::driver::ChannelDriver;

pub struct ChannelRegistry {
    drivers: HashMap<String, Box<dyn ChannelDriver>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self { drivers: HashMap::new() }
    }

    pub fn register(&mut self, driver: Box<dyn ChannelDriver>) {
        self.drivers.insert(driver.platform().to_string(), driver);
    }

    pub fn get(&self, platform: &str) -> Option<&dyn ChannelDriver> {
        self.drivers.get(platform).map(|b| b.as_ref())
    }

    pub fn list_platforms(&self) -> Vec<String> {
        let mut platforms: Vec<String> = self.drivers.keys().cloned().collect();
        platforms.sort();
        platforms
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct TestDriver(&'static str);

    #[async_trait]
    impl ChannelDriver for TestDriver {
        fn platform(&self) -> &'static str {
            self.0
        }

        async fn verify_signature(
            &self,
            _headers: &HashMap<String, String>,
            _raw_body: &[u8],
        ) -> Result<(), crate::driver::ChannelError> {
            Ok(())
        }

        async fn parse_event(
            &self,
            _raw_body: &[u8],
        ) -> Result<crate::driver::ChannelEnvelope, crate::driver::ChannelError> {
            Err(crate::driver::ChannelError::InvalidPayload("not used".to_string()))
        }

        async fn normalize_command(
            &self,
            _env: &crate::driver::ChannelEnvelope,
        ) -> Result<crate::driver::NormalizedCommand, crate::driver::ChannelError> {
            Err(crate::driver::ChannelError::InvalidPayload("not used".to_string()))
        }

        async fn send_message(
            &self,
            _chat_id: &str,
            _text: &str,
        ) -> Result<(), crate::driver::ChannelError> {
            Ok(())
        }
    }

    #[test]
    fn list_platforms_is_sorted() {
        let mut registry = ChannelRegistry::new();
        registry.register(Box::new(TestDriver("wecom")));
        registry.register(Box::new(TestDriver("dingtalk")));
        assert_eq!(registry.list_platforms(), vec!["dingtalk".to_string(), "wecom".to_string()]);
    }
}
