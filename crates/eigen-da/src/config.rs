use {String, ToString};
use core::time::Duration;

/// The EigenDa configuration
pub struct EigenDaConfig {
    /// The url of EigenDA Proxy service
    pub proxy_url: String,
    /// EigenDA Disperser RPC URL
    pub disperse_url: String,
    /// The total amount of time that the batcher will spend waiting for EigenDA to disperse a blob
    pub disperse_blob_timeout: Duration,
    /// The total amount of time that the batcher will spend waiting for EigenDA to retrieve a blob
    pub retrieve_blob_timeout: Duration,
}

/// Need to manually implement Default
impl Default for EigenDaConfig {
    fn default() -> Self {
        Self {
            proxy_url: "http://127.0.0.1:3100".to_string(),
            disperse_url: "".to_string(),
            disperse_blob_timeout: Duration::from_secs(120),
            retrieve_blob_timeout: Duration::from_secs(120),
        }
    }
}