use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::drivers::{DriverRegistry, ProviderDriver};

#[derive(Default)]
pub struct InMemoryDriverRegistry {
    drivers: RwLock<HashMap<String, Arc<dyn ProviderDriver>>>,
}

impl InMemoryDriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, driver: Arc<dyn ProviderDriver>) {
        let mut guard = self.drivers.write().expect("driver registry poisoned");
        guard.insert(driver.driver_id().to_string(), driver);
    }
}

impl DriverRegistry for InMemoryDriverRegistry {
    fn get(&self, driver_id: &str) -> Option<Arc<dyn ProviderDriver>> {
        let guard = self.drivers.read().expect("driver registry poisoned");
        guard.get(driver_id).cloned()
    }
}
