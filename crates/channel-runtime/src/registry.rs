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
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}
