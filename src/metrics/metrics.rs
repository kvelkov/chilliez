use log::info;

pub struct Metrics;

impl Metrics {
    pub fn new() -> Self {
        Metrics {}
    }

    pub fn log_pools_fetched(&self, count: usize) {
        info!("Fetched {} pools from DEXs", count);
    }
}
