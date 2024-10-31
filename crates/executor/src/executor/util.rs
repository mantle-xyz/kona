//! Contains utilities for the L2 executor.

use crate::{constants::HOLOCENE_EXTRA_DATA_VERSION, ExecutorError, ExecutorResult};
use alloc::vec::Vec;
use alloy_consensus::{Eip658Value, Header, Receipt, ReceiptWithBloom};
use alloy_eips::eip1559::BaseFeeParams;
use alloy_primitives::{logs_bloom, Bytes, Log, B64};
use op_alloy_consensus::{
    OpDepositReceipt, OpDepositReceiptWithBloom, OpReceiptEnvelope, OpTxType,
};
use op_alloy_genesis::RollupConfig;
use op_alloy_rpc_types_engine::OpPayloadAttributes;

/// Constructs a [OpReceiptEnvelope] from a [Receipt] fields and [OpTxType].
pub(crate) fn receipt_envelope_from_parts<'a>(
    status: bool,
    cumulative_gas_used: u128,
    logs: impl IntoIterator<Item = &'a Log>,
    tx_type: OpTxType,
    deposit_nonce: Option<u64>,
) -> OpReceiptEnvelope {
    let logs = logs.into_iter().cloned().collect::<Vec<_>>();
    let logs_bloom = logs_bloom(&logs);
    let inner_receipt = Receipt { status: Eip658Value::Eip658(status), cumulative_gas_used, logs };
    match tx_type {
        OpTxType::Legacy => {
            OpReceiptEnvelope::Legacy(ReceiptWithBloom { receipt: inner_receipt, logs_bloom })
        }
        OpTxType::Eip2930 => {
            OpReceiptEnvelope::Eip2930(ReceiptWithBloom { receipt: inner_receipt, logs_bloom })
        }
        OpTxType::Eip1559 => {
            OpReceiptEnvelope::Eip1559(ReceiptWithBloom { receipt: inner_receipt, logs_bloom })
        }
        OpTxType::Eip7702 => panic!("EIP-7702 is not supported"),
        OpTxType::Deposit => {
            let inner = OpDepositReceiptWithBloom {
                receipt: OpDepositReceipt {
                    inner: inner_receipt,
                    deposit_nonce,
                },
                logs_bloom,
            };
            OpReceiptEnvelope::Deposit(inner)
        }
    }
}

/// Parse Holocene [Header] extra data.
///
/// ## Takes
/// - `extra_data`: The extra data field of the [Header].
///
/// ## Returns
/// - `Ok(BaseFeeParams)`: The EIP-1559 parameters.
/// - `Err(ExecutorError::InvalidExtraData)`: If the extra data is invalid.
pub(crate) fn decode_holocene_eip_1559_params(header: &Header) -> ExecutorResult<BaseFeeParams> {
    // Check the extra data length.
    if header.extra_data.len() != 1 + 8 {
        return Err(ExecutorError::InvalidExtraData);
    }

    // Check the extra data version byte.
    if header.extra_data[0] != HOLOCENE_EXTRA_DATA_VERSION {
        return Err(ExecutorError::InvalidExtraData);
    }

    // Parse the EIP-1559 parameters.
    let data = &header.extra_data[1..];
    let denominator =
        u32::from_be_bytes(data[..4].try_into().map_err(|_| ExecutorError::InvalidExtraData)?)
            as u128;
    let elasticity =
        u32::from_be_bytes(data[4..].try_into().map_err(|_| ExecutorError::InvalidExtraData)?)
            as u128;

    // Check for potential division by zero.
    if denominator == 0 {
        return Err(ExecutorError::InvalidExtraData);
    }

    Ok(BaseFeeParams { elasticity_multiplier: elasticity, max_change_denominator: denominator })
}










