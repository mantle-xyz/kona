//! Optimism EVM System calls.

mod eip2935;
pub(crate) use eip2935::pre_block_block_hash_contract_call;

mod eip4788;
pub(crate) use eip4788::pre_block_beacon_root_contract_call;

