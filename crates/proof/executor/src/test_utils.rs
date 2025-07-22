//! Test utilities for the executor.

use crate::{StatelessL2Builder, TrieDBProvider};
use alloy_consensus::Header;
use alloy_op_evm::OpEvmFactory;
use alloy_primitives::{B256, Bytes, Sealable};
use alloy_provider::{Provider, RootProvider, network::primitives::BlockTransactions};
use alloy_rlp::Decodable;
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_engine::PayloadAttributes;
use alloy_transport_http::{Client, Http};
use kona_genesis::RollupConfig;
use kona_mpt::{NoopTrieHinter, TrieNode, TrieProvider};
use op_alloy_rpc_types_engine::OpPayloadAttributes;
use rocksdb::{DB, Options};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use tokio::{fs, runtime::Handle, sync::Mutex};
use tracing::{info, warn};

/// Executes a [ExecutorTestFixture] stored at the passed `fixture_path` and asserts that the
/// produced block hash matches the expected block hash.
pub async fn run_test_fixture(fixture_path: PathBuf) {
    // First, untar the fixture.
    let fixture_dir = tempfile::tempdir().expect("Failed to create temporary directory");
    tokio::process::Command::new("tar")
        .arg("-xvf")
        .arg(fixture_path.as_path())
        .arg("-C")
        .arg(fixture_dir.path())
        .arg("--strip-components=1")
        .output()
        .await
        .expect("Failed to untar fixture");

    let mut options = Options::default();
    options.set_compression_type(rocksdb::DBCompressionType::Snappy);
    options.create_if_missing(true);
    let kv_store = DB::open(&options, fixture_dir.path().join("kv"))
        .unwrap_or_else(|e| panic!("Failed to open database at {fixture_dir:?}: {e}"));
    let provider = DiskTrieNodeProvider::new(kv_store);
    let fixture: ExecutorTestFixture =
        serde_json::from_slice(&fs::read(fixture_dir.path().join("fixture.json")).await.unwrap())
            .expect("Failed to deserialize fixture");

    let mut executor = StatelessL2Builder::new(
        &fixture.rollup_config,
        OpEvmFactory::default(),
        provider,
        NoopTrieHinter,
        fixture.parent_header.seal_slow(),
    );

    let outcome = executor.build_block(fixture.executing_payload).unwrap();

    assert_eq!(
        outcome.header.hash(),
        fixture.expected_block_hash,
        "Produced header does not match the expected header"
    );
}

/// The test fixture format for the [`StatelessL2Builder`].
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutorTestFixture {
    /// The rollup configuration for the executing chain.
    pub rollup_config: RollupConfig,
    /// The parent block header.
    pub parent_header: Header,
    /// The executing payload attributes.
    pub executing_payload: OpPayloadAttributes,
    /// The expected block hash
    pub expected_block_hash: B256,
}

/// A test fixture creator for the [`StatelessL2Builder`].
#[derive(Debug)]
pub struct ExecutorTestFixtureCreator {
    /// The RPC provider for the L2 execution layer.
    pub provider: RootProvider,
    /// The block number to create the test fixture for.
    pub block_number: u64,
    /// The key value store for the test fixture.
    pub kv_store: Arc<Mutex<rocksdb::DB>>,
    /// The data directory for the test fixture.
    pub data_dir: PathBuf,
}

impl ExecutorTestFixtureCreator {
    /// Creates a new [`ExecutorTestFixtureCreator`] with the given parameters.
    pub fn new(provider_url: &str, block_number: u64, base_fixture_directory: PathBuf) -> Self {
        let base = base_fixture_directory.join(format!("block-{}", block_number));

        let url = provider_url.parse().expect("Invalid provider URL");
        let http = Http::<Client>::new(url);
        let provider = RootProvider::new(RpcClient::new(http, false));

        let mut options = Options::default();
        options.set_compression_type(rocksdb::DBCompressionType::Snappy);
        options.create_if_missing(true);
        let db = DB::open(&options, base.join("kv").as_path())
            .unwrap_or_else(|e| panic!("Failed to open database at {base:?}: {e}"));

        Self { provider, block_number, kv_store: Arc::new(Mutex::new(db)), data_dir: base }
    }
}

fn mock_rollup_config() -> RollupConfig {
    let mut rollup_config = RollupConfig { l2_chain_id: 561113, ..Default::default() };
    rollup_config.mantle_skadi_time = Some(0);
    rollup_config
}

impl ExecutorTestFixtureCreator {
    /// Create a static test fixture with the configuration provided.
    pub async fn create_static_fixture(self) -> Result<bool, TestTrieNodeProviderError> {
        // let chain_id = self.provider.get_chain_id().await.expect("Failed to get chain ID");
        let rollup_config = mock_rollup_config();

        let executing_block = match self
            .provider
            .get_block_by_number(self.block_number.into())
            .await
        {
            Ok(Some(block)) => block,
            Ok(None) => {
                warn!(
                    target: "kona_executor::test_utils",
                    block_number = self.block_number,
                    "Block not found"
                );
                return Err(TestTrieNodeProviderError::PreimageNotFound);
            }
            Err(e) => {
                warn!(
                    target: "kona_executor::test_utils",
                    block_number = self.block_number,
                    error = ?e,
                    "Failed to get executing block"
                );
                return Err(TestTrieNodeProviderError::PreimageNotFound);
            }
        };
        
        let parent_block = match self
            .provider
            .get_block_by_number((self.block_number - 1).into())
            .await
        {
            Ok(Some(block)) => block,
            Ok(None) => {
                warn!(
                    target: "kona_executor::test_utils",
                    block_number = self.block_number - 1,
                    "Parent block not found"
                );
                return Err(TestTrieNodeProviderError::PreimageNotFound);
            }
            Err(e) => {
                warn!(
                    target: "kona_executor::test_utils",
                    block_number = self.block_number - 1,
                    error = ?e,
                    "Failed to get parent block"
                );
                return Err(TestTrieNodeProviderError::PreimageNotFound);
            }
        };
        
        let executing_header = executing_block.header;
        let parent_header = parent_block.header.inner.seal_slow();

        let encoded_executing_transactions = match executing_block.transactions {
            BlockTransactions::Hashes(transactions) => {
                let mut encoded_transactions = Vec::with_capacity(transactions.len());
                info!(
                    target: "kona_executor::test_utils",
                    tx_count = transactions.len(),
                    "Processing transactions"
                );
                
                for (i, tx_hash) in transactions.iter().enumerate() {
                    match self
                        .provider
                        .client()
                        .request::<&[B256; 1], Bytes>("debug_getRawTransaction", &[*tx_hash])
                        .await
                    {
                        Ok(tx) => {
                            encoded_transactions.push(tx);
                        }
                        Err(e) => {
                            warn!(
                                target: "kona_executor::test_utils",
                                tx_index = i,
                                tx_hash = ?tx_hash,
                                error = ?e,
                                "Failed to get raw transaction"
                            );
                            return Err(TestTrieNodeProviderError::PreimageNotFound);
                        }
                    }
                }
                encoded_transactions
            }
            _ => {
                warn!(
                    target: "kona_executor::test_utils",
                    "Only BlockTransactions::Hashes are supported"
                );
                panic!("Only BlockTransactions::Hashes are supported.");
            }
        };

        let payload_attrs = OpPayloadAttributes {
            payload_attributes: PayloadAttributes {
                timestamp: executing_header.timestamp,
                parent_beacon_block_root: executing_header.parent_beacon_block_root,
                prev_randao: executing_header.mix_hash,
                withdrawals: Default::default(),
                suggested_fee_recipient: executing_header.beneficiary,
            },
            gas_limit: Some(executing_header.gas_limit),
            transactions: Some(encoded_executing_transactions),
            no_tx_pool: Some(true),
            eip_1559_params: None,
        };

        info!(
            target: "kona_executor::test_utils",
            "Creating executor and building block"
        );

        let mut executor = StatelessL2Builder::new(
            &rollup_config,
            OpEvmFactory::default(),
            self,
            NoopTrieHinter,
            parent_header,
        );
        
        let outcome = match executor.build_block(payload_attrs) {
            Ok(outcome) => outcome,
            Err(e) => {
                warn!(
                    target: "kona_executor::test_utils",
                    error = ?e,
                    "Block execution failed"
                );
                return Err(TestTrieNodeProviderError::ExecutionFailed);
            }
        };

        let success = *outcome.header.inner() == executing_header.inner;
        info!(
            target: "kona_executor::test_utils",
            executing = ?executing_header.inner,
            result = ?outcome.header.inner(),
            success = success,
            "Block execution completed"
        );
        Ok(success)
    }
}

impl TrieProvider for ExecutorTestFixtureCreator {
    type Error = TestTrieNodeProviderError;

    fn trie_node_by_hash(&self, key: B256) -> Result<TrieNode, Self::Error> {
        // Fetch the preimage from the L2 chain provider.
        let preimage: Bytes = tokio::task::block_in_place(move || {
            Handle::current().block_on(async {
                let preimage_result: Result<Bytes, _> = self
                    .provider
                    .client()
                    .request("debug_dbGet", &[key])
                    .await;
                
                let preimage = match preimage_result {
                    Ok(data) => data,
                    Err(e) => {
                        warn!(
                            target: "kona_executor::test_utils",
                            key = ?key,
                            error = ?e,
                            "Failed to get trie node preimage from debug_dbGet"
                        );
                        return Err(TestTrieNodeProviderError::PreimageNotFound);
                    }
                };

                // Store the preimage in the KV store for caching
                if let Err(e) = self.kv_store.lock().await.put(key, preimage.clone()) {
                    warn!(
                        target: "kona_executor::test_utils",
                        key = ?key,
                        error = ?e,
                        "Failed to store preimage in KV store"
                    );
                    return Err(TestTrieNodeProviderError::KVStore);
                }

                Ok(preimage)
            })
        })?;

        // Decode the preimage into a trie node.
        TrieNode::decode(&mut preimage.as_ref()).map_err(|e| {
            warn!(
                target: "kona_executor::test_utils",
                key = ?key,
                error = ?e,
                "Failed to decode trie node from preimage"
            );
            TestTrieNodeProviderError::Rlp(e)
        })
    }
}

impl TrieDBProvider for ExecutorTestFixtureCreator {
    fn bytecode_by_hash(&self, hash: B256) -> Result<Bytes, Self::Error> {
        // geth hashdb scheme code hash key prefix
        const CODE_PREFIX: u8 = b'c';

        // Fetch the preimage from the L2 chain provider.
        let preimage: Bytes = tokio::task::block_in_place(move || {
            Handle::current().block_on(async {
                // Attempt to fetch the code from the L2 chain provider.
                let code_hash = [&[CODE_PREFIX], hash.as_slice()].concat();
                let code_result = self
                    .provider
                    .client()
                    .request::<&[Bytes; 1], Bytes>("debug_dbGet", &[code_hash.into()])
                    .await;

                // Check if the first attempt to fetch the code failed. If it did, try fetching the
                // code hash preimage without the geth hashdb scheme prefix.
                let code = match code_result {
                    Ok(code) => code,
                    Err(e) => {
                        warn!(
                            target: "kona_executor::test_utils",
                            hash = ?hash,
                            error = ?e,
                            "Failed to get bytecode with prefix, trying without prefix"
                        );
                        
                        match self
                            .provider
                            .client()
                            .request::<&[B256; 1], Bytes>("debug_dbGet", &[hash])
                            .await
                        {
                            Ok(code) => code,
                            Err(e2) => {
                                warn!(
                                    target: "kona_executor::test_utils",
                                    hash = ?hash,
                                    error = ?e2,
                                    "Failed to get bytecode without prefix"
                                );
                                return Err(TestTrieNodeProviderError::PreimageNotFound);
                            }
                        }
                    }
                };

                // Store the bytecode in the KV store for caching
                if let Err(e) = self.kv_store.lock().await.put(hash, code.clone()) {
                    warn!(
                        target: "kona_executor::test_utils",
                        hash = ?hash,
                        error = ?e,
                        "Failed to store bytecode in KV store"
                    );
                    return Err(TestTrieNodeProviderError::KVStore);
                }

                Ok(code)
            })
        })?;

        Ok(preimage)
    }

    fn header_by_hash(&self, hash: B256) -> Result<Header, Self::Error> {
        let encoded_header: Bytes = tokio::task::block_in_place(move || {
            Handle::current().block_on(async {
                let header_result: Result<Bytes, _> = self
                    .provider
                    .client()
                    .request("debug_getRawHeader", &[hash])
                    .await;
                
                let preimage = match header_result {
                    Ok(data) => data,
                    Err(e) => {
                        warn!(
                            target: "kona_executor::test_utils",
                            hash = ?hash,
                            error = ?e,
                            "Failed to get header preimage from debug_getRawHeader"
                        );
                        return Err(TestTrieNodeProviderError::PreimageNotFound);
                    }
                };

                // Store the header in the KV store for caching
                if let Err(e) = self.kv_store.lock().await.put(hash, preimage.clone()) {
                    warn!(
                        target: "kona_executor::test_utils",
                        hash = ?hash,
                        error = ?e,
                        "Failed to store header in KV store"
                    );
                    return Err(TestTrieNodeProviderError::KVStore);
                }

                Ok(preimage)
            })
        })?;

        // Decode the Header.
        Header::decode(&mut encoded_header.as_ref()).map_err(|e| {
            warn!(
                target: "kona_executor::test_utils",
                hash = ?hash,
                error = ?e,
                "Failed to decode header from preimage"
            );
            TestTrieNodeProviderError::Rlp(e)
        })
    }
}

/// A simple [`TrieDBProvider`] that reads data from a disk-based key-value store.
#[derive(Debug)]
pub struct DiskTrieNodeProvider {
    kv_store: DB,
}

impl DiskTrieNodeProvider {
    /// Creates a new [`DiskTrieNodeProvider`] with the given [`rocksdb`] K/V store.
    pub const fn new(kv_store: DB) -> Self {
        Self { kv_store }
    }
}

impl TrieProvider for DiskTrieNodeProvider {
    type Error = TestTrieNodeProviderError;

    fn trie_node_by_hash(&self, key: B256) -> Result<TrieNode, Self::Error> {
        TrieNode::decode(
            &mut self
                .kv_store
                .get(key)
                .map_err(|_| TestTrieNodeProviderError::PreimageNotFound)?
                .ok_or(TestTrieNodeProviderError::PreimageNotFound)?
                .as_slice(),
        )
        .map_err(TestTrieNodeProviderError::Rlp)
    }
}

impl TrieDBProvider for DiskTrieNodeProvider {
    fn bytecode_by_hash(&self, code_hash: B256) -> Result<Bytes, Self::Error> {
        self.kv_store
            .get(code_hash)
            .map_err(|_| TestTrieNodeProviderError::PreimageNotFound)?
            .map(Bytes::from)
            .ok_or(TestTrieNodeProviderError::PreimageNotFound)
    }

    fn header_by_hash(&self, hash: B256) -> Result<Header, Self::Error> {
        Header::decode(
            &mut self
                .kv_store
                .get(hash)
                .map_err(|_| TestTrieNodeProviderError::PreimageNotFound)?
                .ok_or(TestTrieNodeProviderError::PreimageNotFound)?
                .as_slice(),
        )
        .map_err(TestTrieNodeProviderError::Rlp)
    }
}

/// An error type for the [`DiskTrieNodeProvider`] and [`ExecutorTestFixtureCreator`].
#[derive(Debug, thiserror::Error)]
pub enum TestTrieNodeProviderError {
    /// The preimage was not found in the key-value store.
    #[error("Preimage not found")]
    PreimageNotFound,
    /// Failed to decode the RLP-encoded data.
    #[error("Failed to decode RLP: {0}")]
    Rlp(alloy_rlp::Error),
    /// Failed to write back to the key-value store.
    #[error("Failed to write back to key value store")]
    KVStore,
    /// Failed to execute the block
    #[error("Failed to execute the block")]
    ExecutionFailed,
}
