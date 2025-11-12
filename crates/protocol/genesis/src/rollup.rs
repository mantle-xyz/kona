//! Rollup Config Types

use crate::ChainGenesis;
use alloy_hardforks::{EthereumHardfork, EthereumHardforks, ForkCondition};
use alloy_op_hardforks::{OpHardfork, OpHardforks};
use alloy_primitives::Address;

/// The max rlp bytes per channel for the Bedrock hardfork.
pub const MAX_RLP_BYTES_PER_CHANNEL_BEDROCK: u64 = 10_000_000;

/// The Rollup configuration.
#[derive(Debug, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct RollupConfig {
    /// The genesis state of the rollup.
    pub genesis: ChainGenesis,
    /// The block time of the L2, in seconds.
    pub block_time: u64,
    /// Sequencer batches may not be more than MaxSequencerDrift seconds after
    /// the L1 timestamp of the sequencing window end.
    ///
    /// Note: When L1 has many 1 second consecutive blocks, and L2 grows at fixed 2 seconds,
    /// the L2 time may still grow beyond this difference.
    ///
    /// Note: After the Fjord hardfork, this value becomes a constant of `1800`.
    pub max_sequencer_drift: u64,
    /// The sequencer window size.
    pub seq_window_size: u64,
    /// Number of L1 blocks between when a channel can be opened and when it can be closed.
    pub channel_timeout: u64,
    /// The L1 chain ID
    pub l1_chain_id: u64,
    /// The L2 chain ID
    pub l2_chain_id: u64,
    /// `regolith_time` sets the activation time of the Regolith network-upgrade:
    /// a pre-mainnet Bedrock change that addresses findings of the Sherlock contest related to
    /// deposit attributes. "Regolith" is the loose deposited rock that sits on top of Bedrock.
    /// Active if regolith_time != None && L2 block timestamp >= Some(regolith_time), inactive
    /// otherwise.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub regolith_time: Option<u64>,
    /// BaseFeeTime sets the activation time of the BaseFee network-upgrade:
    /// Active if BaseFeeTime != nil && L2 block tmestamp >= *BaseFeeTime, inactive otherwise.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub base_fee_time: Option<u64>,
    /// MantleSkadiTime sets the activation time of the skadi network-upgrade:
    /// Mantle only
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub mantle_skadi_time: Option<u64>,
    /// `batch_inbox_address` is the L1 address that batches are sent to.
    pub batch_inbox_address: Address,
    /// `deposit_contract_address` is the L1 address that deposits are sent to.
    pub deposit_contract_address: Address,
    /// `l1_system_config_address` is the L1 address that the system config is stored at.
    pub l1_system_config_address: Address,
    /// `mantle_da_switch` is a switch that weather use mantle da.
    pub mantle_da_switch: bool,
    /// `datalayr_service_manager_addr` is the mantle da manager address that the data availability
    /// contract.
    pub datalayr_service_manager_addr: Address,
}

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for RollupConfig {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            genesis: ChainGenesis::arbitrary(u)?,
            block_time: u.arbitrary()?,
            max_sequencer_drift: u.arbitrary()?,
            seq_window_size: u.arbitrary()?,
            channel_timeout: u.arbitrary()?,
            l1_chain_id: u.arbitrary()?,
            l2_chain_id: u.arbitrary()?,
            regolith_time: u.arbitrary()?,
            base_fee_time: u.arbitrary()?,
            mantle_skadi_time: u.arbitrary()?,
            batch_inbox_address: Address::arbitrary(u)?,
            deposit_contract_address: Address::arbitrary(u)?,
            l1_system_config_address: Address::arbitrary(u)?,
            mantle_da_switch: u.arbitrary()?,
            datalayr_service_manager_addr: Address::arbitrary(u)?,
        })
    }
}

// Need to manually implement Default because [`BaseFeeParams`] has no Default impl.
impl Default for RollupConfig {
    fn default() -> Self {
        Self {
            genesis: ChainGenesis::default(),
            block_time: 0,
            max_sequencer_drift: 0,
            seq_window_size: 0,
            channel_timeout: 0,
            l1_chain_id: 0,
            l2_chain_id: 0,
            regolith_time: None,
            base_fee_time: None,
            mantle_skadi_time: None,
            batch_inbox_address: Address::ZERO,
            deposit_contract_address: Address::ZERO,
            l1_system_config_address: Address::ZERO,
            mantle_da_switch: false,
            datalayr_service_manager_addr: Address::ZERO,
        }
    }
}

#[cfg(feature = "revm")]
impl RollupConfig {
    /// Returns the active [`op_revm::OpSpecId`] for the executor.
    ///
    /// ## Takes
    /// - `timestamp`: The timestamp of the executing block.
    ///
    /// ## Returns
    /// The active [`op_revm::OpSpecId`] for the executor.
    pub fn spec_id(&self, timestamp: u64) -> op_revm::OpSpecId {
        // Mantle skadi is equivalent to isthmus hardfork.
        if self.is_mantle_skadi_active(timestamp) {
            op_revm::OpSpecId::ISTHMUS
        } else if self.is_regolith_active(timestamp) {
            op_revm::OpSpecId::REGOLITH
        } else {
            op_revm::OpSpecId::BEDROCK
        }
    }
}

impl RollupConfig {
    /// Returns true if Mantle Skadi is active at the given timestamp.
    pub fn is_mantle_skadi_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t)
    }

    /// Returns true if Regolith is active at the given timestamp.
    pub fn is_regolith_active(&self, timestamp: u64) -> bool {
        self.regolith_time.is_some_and(|t| timestamp >= t) || self.is_canyon_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Regolith block.
    pub fn is_first_regolith_block(&self, timestamp: u64) -> bool {
        self.is_regolith_active(timestamp) &&
            !self.is_regolith_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Canyon is active at the given timestamp.
    pub fn is_canyon_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_delta_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Canyon block.
    pub fn is_first_canyon_block(&self, timestamp: u64) -> bool {
        self.is_canyon_active(timestamp) &&
            !self.is_canyon_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Delta is active at the given timestamp.
    pub fn is_delta_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_ecotone_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Delta block.
    pub fn is_first_delta_block(&self, timestamp: u64) -> bool {
        self.is_delta_active(timestamp) &&
            !self.is_delta_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Ecotone is active at the given timestamp.
    pub fn is_ecotone_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_fjord_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Ecotone block.
    pub fn is_first_ecotone_block(&self, timestamp: u64) -> bool {
        self.is_ecotone_active(timestamp) &&
            !self.is_ecotone_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Fjord is active at the given timestamp.
    pub fn is_fjord_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_granite_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Fjord block.
    pub fn is_first_fjord_block(&self, timestamp: u64) -> bool {
        self.is_fjord_active(timestamp) &&
            !self.is_fjord_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Granite is active at the given timestamp.
    pub fn is_granite_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_holocene_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Granite block.
    pub fn is_first_granite_block(&self, timestamp: u64) -> bool {
        self.is_granite_active(timestamp) &&
            !self.is_granite_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Holocene is active at the given timestamp.
    pub fn is_holocene_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_isthmus_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Holocene block.
    pub fn is_first_holocene_block(&self, timestamp: u64) -> bool {
        self.is_holocene_active(timestamp) &&
            !self.is_holocene_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if the pectra blob schedule is active at the given timestamp.
    pub fn is_pectra_blob_schedule_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t)
    }

    /// Returns true if the timestamp marks the first pectra blob schedule block.
    pub fn is_first_pectra_blob_schedule_block(&self, timestamp: u64) -> bool {
        self.is_pectra_blob_schedule_active(timestamp) &&
            !self.is_pectra_blob_schedule_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Isthmus is active at the given timestamp.
    pub fn is_isthmus_active(&self, timestamp: u64) -> bool {
        self.mantle_skadi_time.is_some_and(|t| timestamp >= t) || self.is_interop_active(timestamp)
    }

    /// Returns true if the timestamp marks the first Isthmus block.
    pub fn is_first_isthmus_block(&self, timestamp: u64) -> bool {
        self.is_isthmus_active(timestamp) &&
            !self.is_isthmus_active(timestamp.saturating_sub(self.block_time))
    }

    /// Returns true if Jovian is active at the given timestamp.
    pub const fn is_jovian_active(&self, _timestamp: u64) -> bool {
        false
    }

    /// Returns true if the timestamp marks the first Jovian block.
    pub const fn is_first_jovian_block(&self, _timestamp: u64) -> bool {
        false
    }

    /// Returns true if Interop is active at the given timestamp.
    pub const fn is_interop_active(&self, _timestamp: u64) -> bool {
        false
    }

    /// Returns true if the timestamp marks the first Interop block.
    pub const fn is_first_interop_block(&self, _timestamp: u64) -> bool {
        false
    }

    /// Returns the max sequencer drift for the given timestamp.
    pub const fn max_sequencer_drift(&self, _: u64) -> u64 {
        self.max_sequencer_drift
    }

    /// Returns the max rlp bytes per channel for the given timestamp.
    pub const fn max_rlp_bytes_per_channel(&self, _: u64) -> u64 {
        MAX_RLP_BYTES_PER_CHANNEL_BEDROCK
    }

    /// Returns the channel timeout for the given timestamp.
    pub const fn channel_timeout(&self, _: u64) -> u64 {
        self.channel_timeout
    }

    /// Computes a block number from a timestamp, relative to the L2 genesis time and the block
    /// time.
    ///
    /// This function assumes that the timestamp is aligned with the block time, and uses floor
    /// division in its computation.
    pub const fn block_number_from_timestamp(&self, timestamp: u64) -> u64 {
        timestamp.saturating_sub(self.genesis.l2_time).saturating_div(self.block_time)
    }

    /// Checks the scalar value in Ecotone.
    pub fn check_ecotone_l1_system_config_scalar(scalar: [u8; 32]) -> Result<(), &'static str> {
        let version_byte = scalar[0];
        match version_byte {
            0 => {
                if scalar[1..28] != [0; 27] {
                    return Err("Bedrock scalar padding not empty");
                }
                Ok(())
            }
            1 => {
                if scalar[1..24] != [0; 23] {
                    return Err("Invalid version 1 scalar padding");
                }
                Ok(())
            }
            _ => {
                // ignore the event if it's an unknown scalar format
                Err("Unrecognized scalar version")
            }
        }
    }
}

impl EthereumHardforks for RollupConfig {
    fn ethereum_fork_activation(&self, fork: EthereumHardfork) -> ForkCondition {
        if fork <= EthereumHardfork::Berlin {
            // We assume that OP chains were launched with all forks before Berlin activated.
            ForkCondition::Block(0)
        } else if fork <= EthereumHardfork::Paris {
            // Bedrock activates all hardforks up to Paris.
            self.op_fork_activation(OpHardfork::Bedrock)
        } else if fork <= EthereumHardfork::Shanghai {
            // Canyon activates Shanghai hardfork.
            self.op_fork_activation(OpHardfork::Canyon)
        } else if fork <= EthereumHardfork::Cancun {
            // Ecotone activates Cancun hardfork.
            self.op_fork_activation(OpHardfork::Ecotone)
        } else if fork <= EthereumHardfork::Prague {
            // Isthmus activates Prague hardfork.
            self.op_fork_activation(OpHardfork::Isthmus)
        } else {
            ForkCondition::Never
        }
    }
}

impl OpHardforks for RollupConfig {
    fn op_fork_activation(&self, fork: OpHardfork) -> ForkCondition {
        match fork {
            OpHardfork::Bedrock => ForkCondition::Block(0),
            // For Mantle, if mantle_skadi_time is set, it activates all hardforks up to Isthmus
            OpHardfork::Regolith => self.mantle_skadi_time.map_or_else(
                || self.regolith_time.map(ForkCondition::Timestamp).unwrap_or(ForkCondition::Never),
                ForkCondition::Timestamp,
            ),
            OpHardfork::Canyon => {
                self.mantle_skadi_time.map_or(ForkCondition::Never, ForkCondition::Timestamp)
            }
            OpHardfork::Ecotone => {
                self.mantle_skadi_time.map_or(ForkCondition::Never, ForkCondition::Timestamp)
            }
            OpHardfork::Fjord => {
                self.mantle_skadi_time.map_or(ForkCondition::Never, ForkCondition::Timestamp)
            }
            OpHardfork::Granite => {
                self.mantle_skadi_time.map_or(ForkCondition::Never, ForkCondition::Timestamp)
            }
            OpHardfork::Holocene => {
                self.mantle_skadi_time.map_or(ForkCondition::Never, ForkCondition::Timestamp)
            }
            OpHardfork::Isthmus => {
                self.mantle_skadi_time.map_or(ForkCondition::Never, ForkCondition::Timestamp)
            }
            _ => ForkCondition::Never,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use alloy_primitives::{U256, address};

    #[test]
    fn test_rollup_config() {
        let config = RollupConfig::default();
        assert_eq!(config.is_mantle_skadi_active(0), false);
    }

    #[test]
    fn test_deserialize_reference_rollup_config() {
        let ser_cfg = r#"
        {
        "genesis": {
            "l1": {
                "hash": "0x041dea101b3d09fee3dc566c9de820eca07d9d0e951853257c64c79fe4b90f25",
                "number": 4858225
            },
            "l2": {
                "hash": "0x227de3c9c89eb8b8f88a26a06abe125c0d9c7a95a8213f7c83d098e7391bbde6",
                "number": 325709
            },
            "l2_time": 1702194288,
            "system_config": {
                "batcherAddr": "0x5fb5139834df283b6a4bd7267952f3ea21a573f4",
                "overhead": "0x0000000000000000000000000000000000000000000000000000000000000834",
                "scalar": "0x00000000000000000000000000000000000000000000000000000000000f4240",
                "gasLimit": 1125899906842624,
                "baseFee": 1000000000
            }
        },
        "block_time": 2,
        "max_sequencer_drift": 600,
        "seq_window_size": 3600,
        "channel_timeout": 300,
        "l1_chain_id": 11155111,
        "l2_chain_id": 5003,
        "regolith_time": 0,
        "base_fee_time": 1704891600,
        "mantle_skadi_time": 1752649200,
        "batch_inbox_address": "0xffeeddccbbaa0000000000000000000000000000",
        "deposit_contract_address": "0xb3db4bd5bc225930ed674494f9a4f6a11b8efbc8",
        "l1_system_config_address": "0x04b34526c91424e955d13c7226bc4385e57e6706",
        "mantle_da_switch": true,
        "datalayr_service_manager_addr": "0xd7f17171896461A6EB74f95DF3f9b0D966A8a907"
    }
"#;

        let cfg: RollupConfig = serde_json::from_str(ser_cfg).unwrap();
        assert_eq!(cfg.genesis.system_config.unwrap().base_fee, U256::from(1000000000));
        assert_eq!(cfg.l1_chain_id, 11155111);
        assert_eq!(cfg.l2_chain_id, 5003);
        assert_eq!(cfg.mantle_skadi_time, Some(1752649200));
        assert_eq!(cfg.batch_inbox_address, address!("0xffeeddccbbaa0000000000000000000000000000"));
        assert_eq!(
            cfg.deposit_contract_address,
            address!("0xb3db4bd5bc225930ed674494f9a4f6a11b8efbc8")
        );
        assert_eq!(
            cfg.l1_system_config_address,
            address!("0x04b34526c91424e955d13c7226bc4385e57e6706")
        );
        assert!(cfg.mantle_da_switch);
        assert_eq!(
            cfg.datalayr_service_manager_addr,
            address!("0xd7f17171896461A6EB74f95DF3f9b0D966A8a907")
        );
    }
}
