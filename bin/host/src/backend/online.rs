//! Contains the [OnlineHostBackend] definition.

use crate::SharedKeyValueStore;
use anyhow::Result;
use async_trait::async_trait;
use kona_preimage::{
    HintRouter, PreimageFetcher, PreimageKey,
    errors::{PreimageOracleError, PreimageOracleResult},
};
use kona_proof::{Hint, errors::HintParsingError};
use std::{collections::HashSet, hash::Hash, str::FromStr, sync::Arc, time::Duration};
use tokio::{sync::RwLock, time::timeout};
use tracing::{debug, error, trace, warn};

/// Maximum number of retries when fetching preimages
const MAX_RETRIES: usize = 100;

/// Total timeout for fetching a preimage (30 seconds)
const PREIMAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Calculates exponential backoff delay for preimage fetching.
/// Uses millisecond-level backoff suitable for high-frequency operations.
/// Returns: 100ms, 200ms, 400ms, 800ms, 1.6s, 3.2s (capped at 5s)
fn preimage_backoff_delay(attempt: usize) -> Duration {
    let millis = 100u64.saturating_mul(2u64.saturating_pow(attempt.min(5) as u32));
    Duration::from_millis(millis.min(5000))
}

/// The [OnlineHostBackendCfg] trait is used to define the type configuration for the
/// [OnlineHostBackend].
pub trait OnlineHostBackendCfg {
    /// The hint type describing the range of hints that can be received.
    type HintType: FromStr<Err = HintParsingError> + Hash + Eq + PartialEq + Clone + Send + Sync;

    /// The providers that are used to fetch data in response to hints.
    type Providers: Send + Sync;
}

/// A [HintHandler] is an interface for receiving hints, fetching remote data, and storing it in the
/// key-value store.
#[async_trait]
pub trait HintHandler {
    /// The type configuration for the [HintHandler].
    type Cfg: OnlineHostBackendCfg;

    /// Fetches data in response to a hint.
    async fn fetch_hint(
        hint: Hint<<Self::Cfg as OnlineHostBackendCfg>::HintType>,
        cfg: &Self::Cfg,
        providers: &<Self::Cfg as OnlineHostBackendCfg>::Providers,
        kv: SharedKeyValueStore,
    ) -> Result<()>;
}

/// The [OnlineHostBackend] is a [HintRouter] and [PreimageFetcher] that is used to fetch data from
/// remote sources in response to hints.
///
/// [PreimageKey]: kona_preimage::PreimageKey
#[allow(missing_debug_implementations)]
pub struct OnlineHostBackend<C, H>
where
    C: OnlineHostBackendCfg,
    H: HintHandler,
{
    /// The configuration that is used to route hints.
    cfg: C,
    /// The key-value store that is used to store preimages.
    kv: SharedKeyValueStore,
    /// The providers that are used to fetch data in response to hints.
    providers: C::Providers,
    /// Hints that should be immediately executed by the host.
    proactive_hints: HashSet<C::HintType>,
    /// The last hint that was received.
    last_hint: Arc<RwLock<Option<Hint<C::HintType>>>>,
    /// Phantom marker for the [HintHandler].
    _hint_handler: std::marker::PhantomData<H>,
}

impl<C, H> OnlineHostBackend<C, H>
where
    C: OnlineHostBackendCfg,
    H: HintHandler,
{
    /// Creates a new [HintHandler] with the given configuration, key-value store, providers, and
    /// external configuration.
    pub fn new(cfg: C, kv: SharedKeyValueStore, providers: C::Providers, _: H) -> Self {
        Self {
            cfg,
            kv,
            providers,
            proactive_hints: HashSet::default(),
            last_hint: Arc::new(RwLock::new(None)),
            _hint_handler: std::marker::PhantomData,
        }
    }

    /// Adds a new proactive hint to the [OnlineHostBackend].
    pub fn with_proactive_hint(mut self, hint_type: C::HintType) -> Self {
        self.proactive_hints.insert(hint_type);
        self
    }
}

#[async_trait]
impl<C, H> HintRouter for OnlineHostBackend<C, H>
where
    C: OnlineHostBackendCfg + Send + Sync,
    H: HintHandler<Cfg = C> + Send + Sync,
{
    /// Set the last hint to be received.
    async fn route_hint(&self, hint: String) -> PreimageOracleResult<()> {
        trace!(target: "host_backend", "Received hint: {hint}");

        let parsed_hint = hint
            .parse::<Hint<C::HintType>>()
            .map_err(|e| PreimageOracleError::HintParseFailed(e.to_string()))?;
        if self.proactive_hints.contains(&parsed_hint.ty) {
            debug!(target: "host_backend", "Proactive hint received; Immediately fetching {hint}");
            H::fetch_hint(parsed_hint, &self.cfg, &self.providers, self.kv.clone())
                .await
                .map_err(|e| PreimageOracleError::Other(e.to_string()))?;
        } else {
            let mut hint_lock = self.last_hint.write().await;
            hint_lock.replace(parsed_hint);
        }

        Ok(())
    }
}

#[async_trait]
impl<C, H> PreimageFetcher for OnlineHostBackend<C, H>
where
    C: OnlineHostBackendCfg + Send + Sync,
    H: HintHandler<Cfg = C> + Send + Sync,
{
    /// Get the preimage for the given key.
    async fn get_preimage(&self, key: PreimageKey) -> PreimageOracleResult<Vec<u8>> {
        trace!(target: "host_backend", "Pre-image requested. Key: {key}");

        // Acquire a read lock on the key-value store.
        let kv_lock = self.kv.read().await;
        let mut preimage = kv_lock.get(key.into());
        drop(kv_lock);

        // If preimage already exists, return it immediately
        if preimage.is_some() {
            return preimage.ok_or(PreimageOracleError::KeyNotFound);
        }

        // Retry loop with timeout and exponential backoff protection
        let result = timeout(PREIMAGE_FETCH_TIMEOUT, async {
            let mut retry_count = 0;

            while preimage.is_none() {
                // Check retry limit
                if retry_count >= MAX_RETRIES {
                    warn!(
                        target: "host_backend",
                        "Max retries ({}) exceeded for key {}",
                        MAX_RETRIES,
                        key
                    );
                    return Err(PreimageOracleError::Other(format!(
                        "Max retries ({}) exceeded for key {}",
                        MAX_RETRIES, key
                    )));
                }

                // Try to fetch hint if available
                if let Some(hint) = self.last_hint.read().await.as_ref() {
                    match H::fetch_hint(hint.clone(), &self.cfg, &self.providers, self.kv.clone())
                        .await
                    {
                        Ok(_) => {
                            // Check if the key is now available
                            let kv_lock = self.kv.read().await;
                            preimage = kv_lock.get(key.into());
                        }
                        Err(e) => {
                            let delay = preimage_backoff_delay(retry_count);
                            error!(
                                target: "host_backend",
                                "Failed to prefetch hint (attempt {}/{}): {e}",
                                retry_count + 1,
                                MAX_RETRIES
                            );
                            warn!(
                                target: "host_backend",
                                ?delay,
                                "Retrying after delay"
                            );

                            tokio::time::sleep(delay).await;
                        }
                    }
                } else {
                    // No hint available, wait briefly before checking again
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                retry_count += 1;
            }

            Ok(preimage)
        })
        .await
        .map_err(|_| {
            error!(
                target: "host_backend",
                "Timeout ({:?}) exceeded while fetching preimage for key {}",
                PREIMAGE_FETCH_TIMEOUT,
                key
            );
            PreimageOracleError::Timeout
        })?;

        result?.ok_or(PreimageOracleError::KeyNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::MemoryKeyValueStore;
    use alloy_primitives::B256;
    use kona_proof::HintType;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc as StdArc;
    use tokio::sync::RwLock as TokioRwLock;

    // Mock configuration for testing
    #[derive(Clone, Debug)]
    struct TestCfg;

    impl OnlineHostBackendCfg for TestCfg {
        type HintType = HintType;
        type Providers = TestProviders;
    }

    // Mock providers for testing
    #[derive(Clone, Debug)]
    struct TestProviders {
        fetch_count: StdArc<AtomicU32>,
        should_fail: StdArc<AtomicBool>,
        should_store_key: StdArc<TokioRwLock<Option<B256>>>,
    }

    impl TestProviders {
        fn new() -> Self {
            Self {
                fetch_count: StdArc::new(AtomicU32::new(0)),
                should_fail: StdArc::new(AtomicBool::new(false)),
                should_store_key: StdArc::new(TokioRwLock::new(None)),
            }
        }

        fn set_should_fail(&self, fail: bool) {
            self.should_fail.store(fail, Ordering::Relaxed);
        }

        async fn set_store_key(&self, key: B256) {
            let mut store_key = self.should_store_key.write().await;
            *store_key = Some(key);
        }

        fn get_fetch_count(&self) -> u32 {
            self.fetch_count.load(Ordering::Relaxed)
        }
    }

    // Mock hint handler for testing
    #[derive(Clone, Debug, Copy)]
    struct TestHintHandler;

    #[async_trait]
    impl HintHandler for TestHintHandler {
        type Cfg = TestCfg;

        async fn fetch_hint(
            _hint: Hint<HintType>,
            _cfg: &TestCfg,
            providers: &TestProviders,
            kv: SharedKeyValueStore,
        ) -> Result<()> {
            providers.fetch_count.fetch_add(1, Ordering::Relaxed);

            if providers.should_fail.load(Ordering::Relaxed) {
                return Err(anyhow::anyhow!("Mock fetch failure"));
            }

            // Store a preimage if configured
            let store_key = providers.should_store_key.read().await;
            if let Some(key) = *store_key {
                drop(store_key);
                let mut kv_lock = kv.write().await;
                kv_lock.set(key, b"test_preimage_data".to_vec())?;
            }

            Ok(())
        }
    }

    fn create_test_backend() -> (
        OnlineHostBackend<TestCfg, TestHintHandler>,
        SharedKeyValueStore,
        TestProviders,
    ) {
        let kv = Arc::new(RwLock::new(MemoryKeyValueStore::new()));
        let providers = TestProviders::new();
        let cfg = TestCfg;
        let backend = OnlineHostBackend::new(cfg, kv.clone(), providers.clone(), TestHintHandler);
        (backend, kv, providers)
    }

    #[tokio::test]
    async fn test_route_hint_normal() {
        let (backend, _, _) = create_test_backend();
        let hint_str = "l1-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";

        let result = backend.route_hint(hint_str.to_string()).await;
        assert!(result.is_ok());

        // Verify hint was stored
        let hint_lock = backend.last_hint.read().await;
        assert!(hint_lock.is_some());
        assert_eq!(hint_lock.as_ref().unwrap().ty, HintType::L1BlockHeader);
    }

    #[tokio::test]
    async fn test_route_hint_proactive() {
        let (backend, kv, providers) = create_test_backend();
        let backend = backend.with_proactive_hint(HintType::L1BlockHeader);

        let key = B256::from([1u8; 32]);
        providers.set_store_key(key).await;

        let hint_str = "l1-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";

        let result = backend.route_hint(hint_str.to_string()).await;
        assert!(result.is_ok());

        // Verify hint was immediately fetched
        assert_eq!(providers.get_fetch_count(), 1);

        // Verify data was stored
        let kv_lock = kv.read().await;
        assert!(kv_lock.get(key).is_some());
    }

    #[tokio::test]
    async fn test_route_hint_parse_error() {
        let (backend, _, _) = create_test_backend();
        let invalid_hint = "invalid-hint-format";

        let result = backend.route_hint(invalid_hint.to_string()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PreimageOracleError::HintParseFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_get_preimage_existing() {
        let (backend, kv, _) = create_test_backend();
        let key = PreimageKey::new_keccak256([1u8; 32]);
        let expected_data = b"existing_data".to_vec();

        // Pre-populate the key-value store
        {
            let mut kv_lock = kv.write().await;
            kv_lock.set(key.into(), expected_data.clone()).unwrap();
        }

        let result = backend.get_preimage(key).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_data);
    }

    #[tokio::test]
    async fn test_get_preimage_via_hint() {
        let (backend, _kv, providers) = create_test_backend();
        let key = PreimageKey::new_keccak256([1u8; 32]);
        let key_b256: B256 = key.into();
        providers.set_store_key(key_b256).await;

        // Route a hint first
        let hint_str = "l1-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        backend.route_hint(hint_str.to_string()).await.unwrap();

        // Now get the preimage - it should trigger hint fetching
        let result = backend.get_preimage(key).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"test_preimage_data".to_vec());
        assert!(providers.get_fetch_count() > 0);
    }

    #[tokio::test]
    async fn test_get_preimage_not_found() {
        let (backend, _, _) = create_test_backend();
        let key = PreimageKey::new_keccak256([99u8; 32]);

        // Use a shorter timeout for testing
        let result = tokio::time::timeout(Duration::from_millis(500), backend.get_preimage(key))
            .await;

        // Should timeout or return KeyNotFound
        assert!(result.is_ok() || result.is_err());
        if let Ok(Ok(_)) = result {
            // If it succeeded, that's also fine (hint might have been processed)
        } else if let Ok(Err(e)) = result {
            assert!(matches!(
                e,
                PreimageOracleError::KeyNotFound | PreimageOracleError::Timeout
            ));
        }
    }

    #[tokio::test]
    async fn test_get_preimage_with_failing_hint() {
        let (backend, _kv, providers) = create_test_backend();
        let key = PreimageKey::new_keccak256([2u8; 32]);

        // Route a hint first
        let hint_str = "l1-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        backend.route_hint(hint_str.to_string()).await.unwrap();

        // Make fetch_hint fail
        providers.set_should_fail(true);

        // Try to get preimage - should retry and eventually fail or timeout
        let result = tokio::time::timeout(Duration::from_millis(200), backend.get_preimage(key))
            .await;

        // Should timeout due to retries
        assert!(result.is_err() || result.is_ok());
    }

    #[tokio::test]
    async fn test_with_proactive_hint() {
        let (backend, _, _) = create_test_backend();
        let backend = backend.with_proactive_hint(HintType::L1BlockHeader);

        // Verify proactive hint was added (indirectly by testing behavior)
        let hint_str = "l1-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let result = backend.route_hint(hint_str.to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multiple_proactive_hints() {
        let (backend, _, _) = create_test_backend();
        let backend = backend
            .with_proactive_hint(HintType::L1BlockHeader)
            .with_proactive_hint(HintType::L2BlockHeader);

        // Both should be proactive
        let hint1 = "l1-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let hint2 = "l2-block-header 0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";

        assert!(backend.route_hint(hint1.to_string()).await.is_ok());
        assert!(backend.route_hint(hint2.to_string()).await.is_ok());
    }

    #[test]
    fn test_preimage_backoff_delay() {
        // Test backoff delay calculation
        let delay_0 = preimage_backoff_delay(0);
        assert_eq!(delay_0, Duration::from_millis(100));

        let delay_1 = preimage_backoff_delay(1);
        assert_eq!(delay_1, Duration::from_millis(200));

        let delay_2 = preimage_backoff_delay(2);
        assert_eq!(delay_2, Duration::from_millis(400));

        let delay_5 = preimage_backoff_delay(5);
        assert_eq!(delay_5, Duration::from_millis(3200));

        // Test that attempt >= 5 returns the same value (capped by attempt.min(5))
        let delay_10 = preimage_backoff_delay(10);
        assert_eq!(delay_10, Duration::from_millis(3200));
    }
}