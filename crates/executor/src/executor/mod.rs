//! A stateless block executor for the OP Stack.

use crate::{
    constants::{L2_TO_L1_BRIDGE, OUTPUT_ROOT_VERSION, SHA256_EMPTY},
    db::TrieDB,
    errors::TrieDBError,
    ExecutorError, ExecutorResult, TrieDBProvider,
};
use alloc::vec::Vec;
use alloy_consensus::{
    Header, Sealable, Sealed, Transaction, EMPTY_OMMER_ROOT_HASH, EMPTY_ROOT_HASH,
};
use alloy_eips::eip2718::{Decodable2718, Encodable2718};
use alloy_primitives::{keccak256, logs_bloom, Bytes, Log, B256, U256};
use kona_mpt::{ordered_trie_with_encoder, TrieHinter};
use op_alloy_consensus::{OpReceiptEnvelope, OpTxEnvelope};
use op_alloy_genesis::RollupConfig;
use op_alloy_rpc_types_engine::OpPayloadAttributes;
use revm::{
    db::{states::bundle_state::BundleRetention, State},
    primitives::{calc_excess_blob_gas, EnvWithHandlerCfg},
    Evm,
};

mod builder;
pub use builder::{KonaHandleRegister, StatelessL2BlockExecutorBuilder};

mod env;

/// The [ExecutionArtifacts] holds the produced block header and receipts from the execution of a
/// block.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct ExecutionArtifacts {
    /// The block header.
    pub block_header: Sealed<Header>,
    /// The receipts generated during execution.
    pub receipts: Vec<OpReceiptEnvelope>,
}

/// The block executor for the L2 client program. Operates off of a [TrieDB] backed [State],
/// allowing for stateless block execution of OP Stack blocks.
#[derive(Debug)]
pub struct StatelessL2BlockExecutor<'a, F, H>
where
    F: TrieDBProvider,
    H: TrieHinter,
{
    /// The [RollupConfig].
    config: &'a RollupConfig,
    /// The inner state database component.
    trie_db: TrieDB<F, H>,
    /// The [KonaHandleRegister] to use during execution.
    handler_register: Option<KonaHandleRegister<F, H>>,
}

impl<'a, F, H> StatelessL2BlockExecutor<'a, F, H>
where
    F: TrieDBProvider,
    H: TrieHinter,
{
    /// Constructs a new [StatelessL2BlockExecutorBuilder] with the given [RollupConfig].
    pub fn builder(
        config: &'a RollupConfig,
        provider: F,
        hinter: H,
    ) -> StatelessL2BlockExecutorBuilder<'a, F, H> {
        StatelessL2BlockExecutorBuilder::new(config, provider, hinter)
    }

    /// Executes the given block, returning the resulting state root.
    ///
    /// ## Steps
    /// 1. Prepare the block environment.
    /// 2. Apply the pre-block EIP-4788 contract call.
    /// 3. Prepare the EVM with the given L2 execution payload in the block environment.
    ///     - Reject any EIP-4844 transactions, as they are not supported on the OP Stack.
    ///     - If the transaction is a deposit, cache the depositor account prior to execution.
    ///     - Construct the EVM with the given configuration.
    ///     - Execute the transaction.
    ///     - Accumulate the gas used by the transaction to the block-scoped cumulative gas used
    ///       counter.
    ///     - Create a receipt envelope for the transaction.
    /// 4. Merge all state transitions into the cache state.
    /// 5. Compute the [state root, transactions root, receipts root, logs bloom] for the processed
    ///    block.
    pub fn execute_payload(
        &mut self,
        payload: OpPayloadAttributes,
    ) -> ExecutorResult<ExecutionArtifacts> {
        // Prepare the `revm` environment.

        let initialized_block_env = Self::prepare_block_env(
            self.revm_spec_id(payload.payload_attributes.timestamp),
            self.trie_db.parent_block_header(),
            &payload,
        )?;
        let initialized_cfg = self.evm_cfg_env(payload.payload_attributes.timestamp);
        let block_number = initialized_block_env.number.to::<u64>();
        let base_fee = initialized_block_env.basefee.to::<u128>();
        let gas_limit = payload.gas_limit.ok_or(ExecutorError::MissingGasLimit)?;
        let transactions =
            payload.transactions.as_ref().ok_or(ExecutorError::MissingTransactions)?;

        info!(
            target: "client_executor",
            "Executing block # {block_number} | Gas limit: {gas_limit} | Tx count: {tx_len}",
            block_number = block_number,
            gas_limit = gas_limit,
            tx_len = transactions.len(),
        );

        let mut state =
            State::builder().with_database(&mut self.trie_db).with_bundle_update().build();

        let mut cumulative_gas_used = 0u64;
        let mut receipts: Vec<OpReceiptEnvelope> = Vec::with_capacity(transactions.len());
        let is_regolith = self.config.is_regolith_active(payload.payload_attributes.timestamp);

        // Construct the block-scoped EVM with the given configuration.
        // The transaction environment is set within the loop for each transaction.
        let mut evm = {
            let mut base = Evm::builder().with_db(&mut state).with_env_with_handler_cfg(
                EnvWithHandlerCfg::new_with_cfg_env(
                    initialized_cfg.clone(),
                    initialized_block_env.clone(),
                    Default::default(),
                ),
            );

            // If a handler register is provided, append it to the base EVM.
            if let Some(handler) = self.handler_register {
                base = base.append_handler_register(handler);
            }

            base.build()
        };

        // let is_isthmus = self.config.is_isthmus_active(payload.payload_attributes.timestamp);

        // Execute the transactions in the payload.
        let decoded_txs = transactions
            .iter()
            .map(|raw_tx| {
                let tx = OpTxEnvelope::decode_2718(&mut raw_tx.as_ref())
                    .map_err(ExecutorError::RLPError)?;
                Ok((tx, raw_tx.as_ref()))
            })
            .collect::<ExecutorResult<Vec<_>>>()?;
        for (transaction, raw_transaction) in decoded_txs {
            // The sum of the transaction’s gas limit, Tg, and the gas utilized in this block prior,
            // must be no greater than the block’s gasLimit.
            let block_available_gas = (gas_limit - cumulative_gas_used) as u128;
            if (transaction.gas_limit() as u128) > block_available_gas
                && (is_regolith || !transaction.is_system_transaction())
            {
                return Err(ExecutorError::BlockGasLimitExceeded);
            }

            // Prevent EIP-7702 transactions pre-isthmus hardfork.
            if matches!(transaction, OpTxEnvelope::Eip7702(_)) {
                return Err(ExecutorError::UnsupportedTransactionType(transaction.tx_type() as u8));
            }

            // Modify the transaction environment with the current transaction.
            evm = evm
                .modify()
                .with_tx_env(Self::prepare_tx_env(&transaction, raw_transaction)?)
                .build();

            // If the transaction is a deposit, cache the depositor account.
            //
            // This only needs to be done post-Regolith, as deposit nonces were not included in
            // Bedrock. In addition, non-deposit transactions do not have deposit
            // nonces.
            let depositor = is_regolith
                .then(|| {
                    if let OpTxEnvelope::Deposit(deposit) = &transaction {
                        evm.db_mut().load_cache_account(deposit.from).ok().cloned()
                    } else {
                        None
                    }
                })
                .flatten();

            // Execute the transaction.
            let tx_hash = keccak256(raw_transaction);
            debug!(
                target: "client_executor",
                "Executing transaction: {tx_hash}",
            );
            let result = evm.transact_commit().map_err(ExecutorError::ExecutionError)?;
            debug!(
                target: "client_executor",
                "Transaction executed: {tx_hash} | Gas used: {gas_used} | Success: {status}",
                gas_used = result.gas_used(),
                status = result.is_success()
            );

            // Accumulate the gas used by the transaction.
            cumulative_gas_used += result.gas_used();
            // Create receipt envelope.
            let receipt = OpReceiptEnvelope::<Log>::from_parts(
                result.is_success(),
                cumulative_gas_used,
                result.logs(),
                transaction.tx_type(),
                depositor
                    .as_ref()
                    .map(|depositor| depositor.account_info().unwrap_or_default().nonce),
                None,
            );
            // Ensure the receipt is not an EIP-7702 receipt.
            if matches!(receipt, OpReceiptEnvelope::Eip7702(_)) {
                panic!(
                    "EIP-7702 receipts are not supported by the fault proof program before Isthmus"
                );
            }
            receipts.push(receipt);
        }

        info!(
            target: "client_executor",
            "Transaction execution complete | Cumulative gas used: {cumulative_gas_used}",
            cumulative_gas_used = cumulative_gas_used
        );

        // Drop the EVM to free the exclusive reference to the database.
        drop(evm);

        // Merge all state transitions into the cache state.
        debug!(target: "client_executor", "Merging state transitions");
        state.merge_transitions(BundleRetention::Reverts);

        // Take the bundle state.
        let bundle = state.take_bundle();

        // Recompute the header roots.
        let state_root = state.database.state_root(&bundle)?;

        let transactions_root = Self::compute_transactions_root(transactions.as_slice());
        let receipts_root = Self::compute_receipts_root(
            &receipts,
            self.config,
            payload.payload_attributes.timestamp,
        );
        debug!(
            target: "client_executor",
            "Computed transactions root: {transactions_root} | receipts root: {receipts_root}",
        );

        // The withdrawals root on OP Stack chains, after Canyon activation, is always the empty
        // root hash.
        // TODO: if Cancun is active, compute the withdrawals root.
        // let withdrawals_root = self
        //     .config
        //     .is_cancun_active(payload.payload_attributes.timestamp)
        //     .then_some(EMPTY_ROOT_HASH);

        // Compute logs bloom filter for the block.
        let logs_bloom = logs_bloom(receipts.iter().flat_map(|receipt| receipt.logs()));

        // Construct the new header.
        let header = Header {
            parent_hash: state.database.parent_block_header().seal(),
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            beneficiary: payload.payload_attributes.suggested_fee_recipient,
            state_root,
            transactions_root,
            receipts_root,
            withdrawals_root: None,
            logs_bloom,
            difficulty: U256::ZERO,
            number: block_number,
            gas_limit,
            gas_used: cumulative_gas_used,
            timestamp: payload.payload_attributes.timestamp,
            mix_hash: payload.payload_attributes.prev_randao,
            nonce: Default::default(),
            base_fee_per_gas: base_fee.try_into().ok(),
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: payload.payload_attributes.parent_beacon_block_root,
            requests_hash: None,
            extra_data: Default::default(),
        }
        .seal_slow();

        info!(
            target: "client_executor",
            "Sealed new header | Hash: {header_hash} | State root: {state_root} | Transactions root: {transactions_root} | Receipts root: {receipts_root}",
            header_hash = header.seal(),
            state_root = header.state_root,
            transactions_root = header.transactions_root,
            receipts_root = header.receipts_root,
        );

        // Update the parent block hash in the state database.
        state.database.set_parent_block_header(header.clone());
        Ok(ExecutionArtifacts { block_header: header, receipts })
    }

    /// Computes the current output root of the executor, based on the parent header and the
    /// state's underlying trie.
    ///
    /// **CONSTRUCTION:**
    /// ```text
    /// output_root = keccak256(version_byte .. payload)
    /// payload = state_root .. withdrawal_storage_root .. latest_block_hash
    /// ```
    ///
    /// ## Returns
    /// - `Ok(output_root)`: The computed output root.
    /// - `Err(_)`: If an error occurred while computing the output root.
    pub fn compute_output_root(&mut self) -> ExecutorResult<B256> {
        // Fetch the L2 to L1 message passer account from the cache or underlying trie.
        let storage_root = match self.trie_db.storage_roots().get(&L2_TO_L1_BRIDGE) {
            Some(storage_root) => storage_root.blind(),
            None => {
                self.trie_db
                    .get_trie_account(&L2_TO_L1_BRIDGE)?
                    .ok_or(TrieDBError::MissingAccountInfo)?
                    .storage_root
            }
        };
        let parent_header = self.trie_db.parent_block_header();

        info!(
            target: "client_executor",
            "Computing output root | Version: {version} | State root: {state_root} | Storage root: {storage_root} | Block hash: {hash}",
            version = OUTPUT_ROOT_VERSION,
            state_root = self.trie_db.parent_block_header().state_root,
            hash = parent_header.seal(),
        );

        // Construct the raw output.
        let mut raw_output = [0u8; 128];
        raw_output[31] = OUTPUT_ROOT_VERSION;
        raw_output[32..64].copy_from_slice(parent_header.state_root.as_ref());
        raw_output[64..96].copy_from_slice(storage_root.as_ref());
        raw_output[96..128].copy_from_slice(parent_header.seal().as_ref());
        let output_root = keccak256(raw_output);

        info!(
            target: "client_executor",
            "Computed output root for block # {block_number} | Output root: {output_root}",
            block_number = parent_header.number,
        );

        // Hash the output and return
        Ok(output_root)
    }

    /// Computes the receipts root from the given set of receipts.
    ///
    /// ## Takes
    /// - `receipts`: The receipts to compute the root for.
    /// - `config`: The rollup config to use for the computation.
    /// - `timestamp`: The timestamp to use for the computation.
    ///
    /// ## Returns
    /// The computed receipts root.
    fn compute_receipts_root(
        receipts: &[OpReceiptEnvelope],
        config: &RollupConfig,
        timestamp: u64,
    ) -> B256 {
        // There is a minor bug in op-geth and op-erigon where in the Regolith hardfork,
        // the receipt root calculation does not inclide the deposit nonce in the
        // receipt encoding. In the Regolith hardfork, we must strip the deposit nonce
        // from the receipt encoding to match the receipt root calculation.
        if config.is_regolith_active(timestamp) {
            let receipts = receipts
                .iter()
                .cloned()
                .map(|receipt| match receipt {
                    OpReceiptEnvelope::Deposit(mut deposit_receipt) => {
                        deposit_receipt.receipt.deposit_nonce = None;
                        OpReceiptEnvelope::Deposit(deposit_receipt)
                    }
                    _ => receipt,
                })
                .collect::<Vec<_>>();

            ordered_trie_with_encoder(receipts.as_ref(), |receipt, mut buf| {
                receipt.encode_2718(&mut buf)
            })
            .root()
        } else {
            ordered_trie_with_encoder(receipts, |receipt, mut buf| receipt.encode_2718(&mut buf))
                .root()
        }
    }

    /// Computes the transactions root from the given set of encoded transactions.
    ///
    /// ## Takes
    /// - `transactions`: The transactions to compute the root for.
    ///
    /// ## Returns
    /// The computed transactions root.
    fn compute_transactions_root(transactions: &[Bytes]) -> B256 {
        ordered_trie_with_encoder(transactions, |tx, buf| buf.put_slice(tx.as_ref())).root()
    }
}

#[cfg(test)]
mod test {
    use crate::test_utils::run_test_fixture;
    use rstest::rstest;
    use std::path::PathBuf;

    // To create new test fixtures, uncomment the following test and run it with parameters filled.
    //
    // #[tokio::test(flavor = "multi_thread")]
    // async fn create_fixture() {
    //     let fixture_creator = crate::test_utils::ExecutorTestFixtureCreator::new(
    //         "<l2_archive_el_rpc_url>",
    //         <block_number>,
    //         PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata"),
    //     );
    //     fixture_creator.create_static_fixture().await;
    // }

    #[rstest]
    #[case::small_block(22884230)]
    #[case::small_block_2(22884231)]
    #[case::small_block_3(22880574)]
    #[case::small_block_4(22887258)]
    #[case::medium_block(22886464)]
    #[case::medium_block_2(22886311)]
    #[case::medium_block_3(22880944)]
    #[tokio::test]
    async fn test_statelessly_execute_block(#[case] block_number: u64) {
        let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(format!("block-{block_number}.tar.gz"));

        run_test_fixture(fixture_dir).await;
    }
}
