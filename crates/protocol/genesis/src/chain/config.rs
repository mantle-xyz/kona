//! Contains the chain config type.

use alloc::string::String;
use alloy_chains::Chain;
use alloy_eips::eip1559::BaseFeeParams;
use alloy_primitives::Address;

use crate::{
    AddressList, AltDAConfig, BaseFeeConfig, ChainGenesis, HardForkConfig, Roles, RollupConfig,
    SuperchainLevel, base_fee_params, base_fee_params_canyon, params::base_fee_config,
};

/// L1 chain configuration from the `alloy-genesis` crate.
pub type L1ChainConfig = alloy_genesis::ChainConfig;

/// Defines core blockchain settings per block.
///
/// Tailors unique settings for each network based on
/// its genesis block and superchain configuration.
///
/// This struct bridges the interface between the [`ChainConfig`][ccr]
/// defined in the [`superchain-registry`][scr] and the [`ChainConfig`][ccg]
/// defined in [`op-geth`][opg].
///
/// [opg]: https://github.com/ethereum-optimism/op-geth
/// [scr]: https://github.com/ethereum-optimism/superchain-registry
/// [ccg]: https://github.com/ethereum-optimism/op-geth/blob/optimism/params/config.go#L342
/// [ccr]: https://github.com/ethereum-optimism/superchain-registry/blob/main/ops/internal/config/superchain.go#L70
#[derive(Debug, Clone, Default, Eq, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ChainConfig {
    /// Chain name (e.g. "Base")
    #[cfg_attr(feature = "serde", serde(rename = "Name", alias = "name"))]
    pub name: String,
    /// L1 chain ID
    #[cfg_attr(feature = "serde", serde(skip))]
    pub l1_chain_id: u64,
    /// Chain public RPC endpoint
    #[cfg_attr(feature = "serde", serde(rename = "PublicRPC", alias = "public_rpc"))]
    pub public_rpc: String,
    /// Chain sequencer RPC endpoint
    #[cfg_attr(feature = "serde", serde(rename = "SequencerRPC", alias = "sequencer_rpc"))]
    pub sequencer_rpc: String,
    /// Chain explorer HTTP endpoint
    #[cfg_attr(feature = "serde", serde(rename = "Explorer", alias = "explorer"))]
    pub explorer: String,
    /// Level of integration with the superchain.
    #[cfg_attr(feature = "serde", serde(rename = "SuperchainLevel", alias = "superchain_level"))]
    pub superchain_level: SuperchainLevel,
    /// Whether the chain is governed by optimism.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "GovernedByOptimism", alias = "governed_by_optimism")
    )]
    #[cfg_attr(feature = "serde", serde(default))]
    pub governed_by_optimism: bool,
    /// Time of when a given chain is opted in to the Superchain.
    /// If set, hardforks times after the superchain time
    /// will be inherited from the superchain-wide config.
    #[cfg_attr(feature = "serde", serde(rename = "SuperchainTime", alias = "superchain_time"))]
    pub superchain_time: Option<u64>,
    /// Data availability type.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "DataAvailabilityType", alias = "data_availability_type")
    )]
    pub data_availability_type: String,
    /// Chain ID
    #[cfg_attr(feature = "serde", serde(rename = "l2_chain_id", alias = "chain_id"))]
    pub chain_id: u64,
    /// Chain-specific batch inbox address
    #[cfg_attr(
        feature = "serde",
        serde(rename = "batch_inbox_address", alias = "batch_inbox_addr")
    )]
    #[cfg_attr(feature = "serde", serde(default))]
    pub batch_inbox_addr: Address,
    /// The block time in seconds.
    #[cfg_attr(feature = "serde", serde(rename = "block_time"))]
    pub block_time: u64,
    /// The sequencer window size in seconds.
    #[cfg_attr(feature = "serde", serde(rename = "seq_window_size"))]
    pub seq_window_size: u64,
    /// The maximum sequencer drift in seconds.
    #[cfg_attr(feature = "serde", serde(rename = "max_sequencer_drift"))]
    pub max_sequencer_drift: u64,
    /// Gas paying token metadata. Not consumed by downstream OPStack components.
    #[cfg_attr(feature = "serde", serde(rename = "GasPayingToken", alias = "gas_paying_token"))]
    pub gas_paying_token: Option<Address>,
    /// Hardfork Config. These values may override the superchain-wide defaults.
    #[cfg_attr(feature = "serde", serde(rename = "hardfork_configuration", alias = "hardforks"))]
    pub hardfork_config: HardForkConfig,
    /// Optimism configuration
    #[cfg_attr(feature = "serde", serde(rename = "optimism"))]
    pub optimism: Option<BaseFeeConfig>,
    /// Alternative DA configuration
    #[cfg_attr(feature = "serde", serde(rename = "alt_da"))]
    pub alt_da: Option<AltDAConfig>,
    /// Chain-specific genesis information
    pub genesis: ChainGenesis,
    /// Roles
    #[cfg_attr(feature = "serde", serde(rename = "Roles", alias = "roles"))]
    pub roles: Option<Roles>,
    /// Addresses
    #[cfg_attr(feature = "serde", serde(rename = "Addresses", alias = "addresses"))]
    pub addresses: Option<AddressList>,
}

impl ChainConfig {
    /// Returns the base fee params for the chain.
    pub fn base_fee_params(&self) -> BaseFeeParams {
        self.optimism
            .as_ref()
            .map(|op| op.as_base_fee_params())
            .unwrap_or_else(|| base_fee_params(self.chain_id))
    }

    /// Returns the canyon base fee params for the chain.
    pub fn canyon_base_fee_params(&self) -> BaseFeeParams {
        self.optimism
            .as_ref()
            .map(|op| op.as_canyon_base_fee_params())
            .unwrap_or_else(|| base_fee_params_canyon(self.chain_id))
    }

    /// Returns the base fee config for the chain.
    pub fn base_fee_config(&self) -> BaseFeeConfig {
        self.optimism.as_ref().map(|op| *op).unwrap_or_else(|| base_fee_config(self.chain_id))
    }

    /// Loads the rollup config for the OP-Stack chain given the chain config and address list.
    #[deprecated(since = "0.2.1", note = "please use `as_rollup_config` instead")]
    pub fn load_op_stack_rollup_config(&self) -> RollupConfig {
        self.as_rollup_config()
    }

    /// Loads the rollup config for the OP-Stack chain given the chain config and address list.
    pub fn as_rollup_config(&self) -> RollupConfig {
        RollupConfig::default()
    }
}
