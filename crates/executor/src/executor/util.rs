//! Contains utilities for the L2 executor.

use crate::{constants::HOLOCENE_EXTRA_DATA_VERSION, ExecutorError, ExecutorResult};
use alloc::vec::Vec;
use alloy_consensus::Header;
use alloy_eips::eip1559::BaseFeeParams;
use alloy_primitives::{Bytes, B64};
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

/// Encode Holocene [Header] extra data.
///
/// ## Takes
/// - `config`: The [RollupConfig] for the chain.
/// - `attributes`: The [OpPayloadAttributes] for the block.
///
/// ## Returns
/// - `Ok(data)`: The encoded extra data.
/// - `Err(ExecutorError::MissingEIP1559Params)`: If the EIP-1559 parameters are missing.
pub(crate) fn encode_holocene_eip_1559_params(
    config: &RollupConfig,
    attributes: &OpPayloadAttributes,
) -> ExecutorResult<Bytes> {
    let payload_params = attributes.eip_1559_params.ok_or(ExecutorError::MissingEIP1559Params)?;
    let params = if payload_params == B64::ZERO {
        encode_canyon_base_fee_params(config)
    } else {
        payload_params
    };

    let mut data = Vec::with_capacity(1 + 8);
    data.push(HOLOCENE_EXTRA_DATA_VERSION);
    data.extend_from_slice(params.as_ref());
    Ok(data.into())
}

/// Encodes the canyon base fee parameters, per Holocene spec.
///
/// <https://specs.optimism.io/protocol/holocene/exec-engine.html#eip1559params-encoding>
pub(crate) fn encode_canyon_base_fee_params(config: &RollupConfig) -> B64 {
    let params = config.canyon_base_fee_params;

    let mut buf = B64::ZERO;
    buf[..4].copy_from_slice(&(params.max_change_denominator as u32).to_be_bytes());
    buf[4..].copy_from_slice(&(params.elasticity_multiplier as u32).to_be_bytes());
    buf
}

#[cfg(test)]
mod test {
    use super::decode_holocene_eip_1559_params;
    use crate::executor::util::{encode_canyon_base_fee_params, encode_holocene_eip_1559_params};
    use alloy_consensus::Header;
    use alloy_eips::eip1559::BaseFeeParams;
    use alloy_primitives::{b64, hex, B64};
    use alloy_rpc_types_engine::PayloadAttributes;
    use op_alloy_genesis::RollupConfig;
    use op_alloy_rpc_types_engine::OpPayloadAttributes;

    fn mock_payload(eip_1559_params: Option<B64>) -> OpPayloadAttributes {
        OpPayloadAttributes {
            payload_attributes: PayloadAttributes {
                timestamp: 0,
                prev_randao: Default::default(),
                suggested_fee_recipient: Default::default(),
                withdrawals: Default::default(),
                parent_beacon_block_root: Default::default(),
                target_blobs_per_block: None,
                max_blobs_per_block: None,
            },
            transactions: None,
            no_tx_pool: None,
            gas_limit: None,
            eip_1559_params,
        }
    }

    #[test]
    fn test_decode_holocene_eip_1559_params() {
        let params = hex!("00BEEFBABE0BADC0DE");
        let mock_header = Header { extra_data: params.to_vec().into(), ..Default::default() };
        let params = decode_holocene_eip_1559_params(&mock_header).unwrap();

        assert_eq!(params.elasticity_multiplier, 0x0BAD_C0DE);
        assert_eq!(params.max_change_denominator, 0xBEEF_BABE);
    }









