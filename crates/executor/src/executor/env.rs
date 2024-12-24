//! Environment preparation for the executor.

use super::{ StatelessL2BlockExecutor};
use crate::{constants::FEE_RECIPIENT, ExecutorError, ExecutorResult, TrieDBProvider};
use alloy_consensus::Header;
use alloy_eips::eip1559::BaseFeeParams;
use alloy_primitives::{TxKind, U256};
use kona_mpt::TrieHinter;
use op_alloy_consensus::OpTxEnvelope;
use op_alloy_genesis::RollupConfig;
use op_alloy_rpc_types_engine::OpPayloadAttributes;
use revm::primitives::{
    BlobExcessGasAndPrice, BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, OptimismFields, SpecId,
    TransactTo, TxEnv,
};

impl<P, H> StatelessL2BlockExecutor<'_, P, H>
where
    P: TrieDBProvider,
    H: TrieHinter,
{
    /// Returns the active [SpecId] for the executor.
    ///
    /// ## Takes
    /// - `timestamp`: The timestamp of the executing block.
    ///
    /// ## Returns
    /// The active [SpecId] for the executor.
    pub(crate) fn revm_spec_id(&self, timestamp: u64) -> SpecId {
        if self.config.is_shanghai_active(timestamp) {
            SpecId::SHANGHAI
        } else if self.config.is_regolith_active(timestamp) {
            SpecId::REGOLITH
        } else {
            SpecId::BEDROCK
        }
    }

    /// Returns the active [CfgEnvWithHandlerCfg] for the executor.
    ///
    /// ## Takes
    /// - `timestamp`: The timestamp of the executing block.
    ///
    /// ## Returns
    /// The active [CfgEnvWithHandlerCfg] for the executor.
    pub(crate) fn evm_cfg_env(&self, timestamp: u64) -> CfgEnvWithHandlerCfg {
        let cfg_env = CfgEnv::default().with_chain_id(self.config.l2_chain_id);
        let mut cfg_handler_env =
            CfgEnvWithHandlerCfg::new_with_spec_id(cfg_env, self.revm_spec_id(timestamp));
        cfg_handler_env.enable_optimism();
        cfg_handler_env
    }

    /// Prepares a [BlockEnv] with the given [OpPayloadAttributes].
    ///
    /// ## Takes
    /// - `spec_id`: The [SpecId] to prepare the environment for.
    /// - `parent_header`: The parent header of the block to be executed.
    /// - `payload_attrs`: The payload to prepare the environment for.
    /// - `base_fee_params`: The active base fee parameters for the block.
    pub(crate) fn prepare_block_env(
        spec_id: SpecId,
        parent_header: &Header,
        payload_attrs: &OpPayloadAttributes,
    ) -> ExecutorResult<BlockEnv> {
        let blob_excess_gas_and_price = parent_header
            .next_block_excess_blob_gas()
            .or_else(|| spec_id.is_enabled_in(SpecId::ECOTONE).then_some(0))
            .map(BlobExcessGasAndPrice::new);
        let next_block_base_fee = parent_header.base_fee_per_gas.unwrap_or_default();

        Ok(BlockEnv {
            number: U256::from(parent_header.number + 1),
            coinbase: FEE_RECIPIENT,
            timestamp: U256::from(payload_attrs.payload_attributes.timestamp),
            gas_limit: U256::from(payload_attrs.gas_limit.ok_or(ExecutorError::MissingGasLimit)?),
            basefee: U256::from(next_block_base_fee),
            difficulty: U256::ZERO,
            prevrandao: Some(payload_attrs.payload_attributes.prev_randao),
            blob_excess_gas_and_price,
        })
    }

    /// Prepares a [TxEnv] with the given [OpTxEnvelope].
    ///
    /// ## Takes
    /// - `transaction`: The transaction to prepare the environment for.
    /// - `env`: The transaction environment to prepare.
    ///
    /// ## Returns
    /// - `Ok(())` if the environment was successfully prepared.
    /// - `Err(_)` if an error occurred while preparing the environment.
    pub(crate) fn prepare_tx_env(
        transaction: &OpTxEnvelope,
        encoded_transaction: &[u8],
    ) -> ExecutorResult<TxEnv> {
        let mut env = TxEnv::default();
        match transaction {
            OpTxEnvelope::Legacy(signed_tx) => {
                let tx = signed_tx.tx();
                env.caller = signed_tx.recover_signer().map_err(ExecutorError::SignatureError)?;
                env.gas_limit = tx.gas_limit;
                env.gas_price = U256::from(tx.gas_price);
                env.gas_priority_fee = None;
                env.transact_to = match tx.to {
                    TxKind::Call(to) => TransactTo::Call(to),
                    TxKind::Create => TransactTo::Create,
                };
                env.value = tx.value;
                env.data = tx.input.clone();
                env.chain_id = tx.chain_id;
                env.nonce = Some(tx.nonce);
                env.access_list.clear();
                env.blob_hashes.clear();
                env.max_fee_per_blob_gas.take();
                env.optimism = OptimismFields {
                    source_hash: None,
                    mint: None,
                    is_system_transaction: Some(false),
                    enveloped_tx: Some(encoded_transaction.to_vec().into()),
                    eth_value: None,
                    eth_tx_value: None,
                };
                Ok(env)
            }
            OpTxEnvelope::Eip2930(signed_tx) => {
                let tx = signed_tx.tx();
                env.caller = signed_tx.recover_signer().map_err(ExecutorError::SignatureError)?;
                env.gas_limit = tx.gas_limit;
                env.gas_price = U256::from(tx.gas_price);
                env.gas_priority_fee = None;
                env.transact_to = match tx.to {
                    TxKind::Call(to) => TransactTo::Call(to),
                    TxKind::Create => TransactTo::Create,
                };
                env.value = tx.value;
                env.data = tx.input.clone();
                env.chain_id = Some(tx.chain_id);
                env.nonce = Some(tx.nonce);
                env.access_list = tx.access_list.to_vec();
                env.blob_hashes.clear();
                env.max_fee_per_blob_gas.take();
                env.optimism = OptimismFields {
                    source_hash: None,
                    mint: None,
                    is_system_transaction: Some(false),
                    enveloped_tx: Some(encoded_transaction.to_vec().into()),
                    eth_value: None,
                    eth_tx_value: None,
                };
                Ok(env)
            }
            OpTxEnvelope::Eip1559(signed_tx) => {
                let tx = signed_tx.tx();
                env.caller = signed_tx.recover_signer().map_err(ExecutorError::SignatureError)?;
                env.gas_limit = tx.gas_limit;
                env.gas_price = U256::from(tx.max_fee_per_gas);
                env.gas_priority_fee = Some(U256::from(tx.max_priority_fee_per_gas));
                env.transact_to = match tx.to {
                    TxKind::Call(to) => TransactTo::Call(to),
                    TxKind::Create => TransactTo::Create,
                };
                env.value = tx.value;
                env.data = tx.input.clone();
                env.chain_id = Some(tx.chain_id);
                env.nonce = Some(tx.nonce);
                env.access_list = tx.access_list.to_vec();
                env.blob_hashes.clear();
                env.max_fee_per_blob_gas.take();
                env.optimism = OptimismFields {
                    source_hash: None,
                    mint: None,
                    is_system_transaction: Some(false),
                    enveloped_tx: Some(encoded_transaction.to_vec().into()),
                    eth_value: None,
                    eth_tx_value: None,
                };
                Ok(env)
            }
            OpTxEnvelope::Deposit(tx) => {
                env.caller = tx.from;
                env.access_list.clear();
                env.gas_limit = tx.gas_limit;
                env.gas_price = U256::ZERO;
                env.gas_priority_fee = None;
                match tx.to {
                    TxKind::Call(to) => env.transact_to = TransactTo::Call(to),
                    TxKind::Create => env.transact_to = TransactTo::Create,
                }
                env.value = tx.value;
                env.data = tx.input.clone();
                env.chain_id = None;
                env.nonce = None;
                env.optimism = OptimismFields {
                    source_hash: Some(tx.source_hash),
                    mint: tx.mint,
                    is_system_transaction: Some(tx.is_system_transaction),
                    enveloped_tx: Some(encoded_transaction.to_vec().into()),
                    eth_value: tx.eth_value,
                    eth_tx_value: tx.eth_tx_value,
                };
                Ok(env)
            }
            _ => Err(ExecutorError::UnsupportedTransactionType(transaction.tx_type() as u8)),
        }
    }
}
