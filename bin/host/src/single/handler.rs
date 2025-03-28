//! [HintHandler] for the [SingleChainHost].

use crate::{
    backend::util::store_ordered_trie, kv::SharedKeyValueStore, single::cfg::SingleChainHost,
    EigenDABlobWitness, HintHandler, OnlineHostBackendCfg,
};
use alloy_consensus::Header;
use alloy_eips::{
    eip2718::Encodable2718,
    eip4844::{IndexedBlobHash, FIELD_ELEMENTS_PER_BLOB},
};
use alloy_primitives::{address, keccak256, Address, Bytes, B256};
use alloy_provider::Provider;
use alloy_rlp::Decodable;
use alloy_rpc_types::{debug::ExecutionWitness, Block, BlockTransactionsKind};
use anyhow::{anyhow, ensure, Result};
use async_trait::async_trait;
use eigen_da::{BlobInfo, EigenDABlobData, BYTES_PER_FIELD_ELEMENT};
use kona_preimage::{PreimageKey, PreimageKeyType};
use kona_proof::{Hint, HintType};
use op_alloy_protocol::BlockInfo;
use op_alloy_rpc_types_engine::OpPayloadAttributes;
use std::collections::HashMap;
use tracing::{debug, info};

/// The [HintHandler] for the [SingleChainHost].
#[derive(Debug, Clone, Copy)]
pub struct SingleChainHintHandler;

#[async_trait]
impl HintHandler for SingleChainHintHandler {
    type Cfg = SingleChainHost;

    async fn fetch_hint(
        hint: Hint<<Self::Cfg as OnlineHostBackendCfg>::HintType>,
        cfg: &Self::Cfg,
        providers: &<Self::Cfg as OnlineHostBackendCfg>::Providers,
        kv: SharedKeyValueStore,
    ) -> Result<()> {
        match hint.ty {
            HintType::L1BlockHeader => {
                ensure!(hint.data.len() == 32, "Invalid hint data length");

                let hash: B256 = hint.data.as_ref().try_into()?;
                let raw_header: Bytes =
                    providers.l1.client().request("debug_getRawHeader", [hash]).await?;

                let mut kv_lock = kv.write().await;
                kv_lock.set(PreimageKey::new_keccak256(*hash).into(), raw_header.into())?;
            }
            HintType::L1Transactions => {
                ensure!(hint.data.len() == 32, "Invalid hint data length");

                let hash: B256 = hint.data.as_ref().try_into()?;
                let Block { transactions, .. } = providers
                    .l1
                    .get_block_by_hash(hash, BlockTransactionsKind::Full)
                    .await?
                    .ok_or(anyhow!("Block not found"))?;
                let encoded_transactions = transactions
                    .into_transactions()
                    .map(|tx| tx.inner.encoded_2718())
                    .collect::<Vec<_>>();

                store_ordered_trie(kv.as_ref(), encoded_transactions.as_slice()).await?;
            }
            HintType::L1Receipts => {
                ensure!(hint.data.len() == 32, "Invalid hint data length");

                let hash: B256 = hint.data.as_ref().try_into()?;
                let raw_receipts: Vec<Bytes> =
                    providers.l1.client().request("debug_getRawReceipts", [hash]).await?;

                store_ordered_trie(kv.as_ref(), raw_receipts.as_slice()).await?;
            }
            HintType::L1Blob => {
                ensure!(hint.data.len() == 48, "Invalid hint data length");

                let hash_data_bytes: [u8; 32] = hint.data[0..32].try_into()?;
                let index_data_bytes: [u8; 8] = hint.data[32..40].try_into()?;
                let timestamp_data_bytes: [u8; 8] = hint.data[40..48].try_into()?;

                let hash: B256 = hash_data_bytes.into();
                let index = u64::from_be_bytes(index_data_bytes);
                let timestamp = u64::from_be_bytes(timestamp_data_bytes);

                let partial_block_ref = BlockInfo { timestamp, ..Default::default() };
                let indexed_hash = IndexedBlobHash { index, hash };

                // Fetch the blob sidecar from the blob provider.
                let mut sidecars = providers
                    .blobs
                    .fetch_filtered_sidecars(&partial_block_ref, &[indexed_hash])
                    .await
                    .map_err(|e| anyhow!("Failed to fetch blob sidecars: {e}"))?;
                if sidecars.len() != 1 {
                    anyhow::bail!("Expected 1 sidecar, got {}", sidecars.len());
                }
                let sidecar = sidecars.remove(0);

                // Acquire a lock on the key-value store and set the preimages.
                let mut kv_lock = kv.write().await;

                // Set the preimage for the blob commitment.
                kv_lock.set(
                    PreimageKey::new(*hash, PreimageKeyType::Sha256).into(),
                    sidecar.kzg_commitment.to_vec(),
                )?;

                // Write all the field elements to the key-value store. There should be 4096.
                // The preimage oracle key for each field element is the keccak256 hash of
                // `abi.encodePacked(sidecar.KZGCommitment, uint256(i))`
                let mut blob_key = [0u8; 80];
                blob_key[..48].copy_from_slice(sidecar.kzg_commitment.as_ref());
                for i in 0..FIELD_ELEMENTS_PER_BLOB {
                    blob_key[72..].copy_from_slice(i.to_be_bytes().as_ref());
                    let blob_key_hash = keccak256(blob_key.as_ref());

                    kv_lock
                        .set(PreimageKey::new_keccak256(*blob_key_hash).into(), blob_key.into())?;
                    kv_lock.set(
                        PreimageKey::new(*blob_key_hash, PreimageKeyType::Blob).into(),
                        sidecar.blob[(i as usize) << 5..(i as usize + 1) << 5].to_vec(),
                    )?;
                }

                // Write the KZG Proof as the 4096th element.
                blob_key[72..].copy_from_slice((FIELD_ELEMENTS_PER_BLOB).to_be_bytes().as_ref());
                let blob_key_hash = keccak256(blob_key.as_ref());
                kv_lock.set(PreimageKey::new_keccak256(*blob_key_hash).into(), blob_key.into())?;
                kv_lock.set(
                    PreimageKey::new(*blob_key_hash, PreimageKeyType::Blob).into(),
                    sidecar.kzg_proof.to_vec(),
                )?;
            }
            HintType::L1Precompile => {
                ensure!(hint.data.len() >= 20, "Invalid hint data length");

                let address = Address::from_slice(&hint.data.as_ref()[..20]);
                let input = hint.data[20..].to_vec();
                let input_hash = keccak256(hint.data.as_ref());

                let result = crate::eth::execute(address, input).map_or_else(
                    |_| vec![0u8; 1],
                    |raw_res| {
                        let mut res = Vec::with_capacity(1 + raw_res.len());
                        res.push(0x01);
                        res.extend_from_slice(&raw_res);
                        res
                    },
                );

                let mut kv_lock = kv.write().await;
                kv_lock.set(PreimageKey::new_keccak256(*input_hash).into(), hint.data.into())?;
                kv_lock.set(
                    PreimageKey::new(*input_hash, PreimageKeyType::Precompile).into(),
                    result,
                )?;
            }
            HintType::L2BlockHeader => {
                ensure!(hint.data.len() == 32, "Invalid hint data length");

                // Fetch the raw header from the L2 chain provider.
                let hash: B256 = hint.data.as_ref().try_into()?;
                let raw_header: Bytes =
                    providers.l2.client().request("debug_getRawHeader", [hash]).await?;

                // Acquire a lock on the key-value store and set the preimage.
                let mut kv_lock = kv.write().await;
                kv_lock.set(PreimageKey::new_keccak256(*hash).into(), raw_header.into())?;
            }
            HintType::L2Transactions => {
                ensure!(hint.data.len() == 32, "Invalid hint data length");

                let hash: B256 = hint.data.as_ref().try_into()?;
                let Block { transactions, .. } = providers
                    .l2
                    .get_block_by_hash(hash, BlockTransactionsKind::Full)
                    .await?
                    .ok_or(anyhow!("Block not found."))?;

                let encoded_transactions = transactions
                    .into_transactions()
                    .map(|tx| tx.inner.inner.encoded_2718())
                    .collect::<Vec<_>>();
                store_ordered_trie(kv.as_ref(), encoded_transactions.as_slice()).await?;
            }
            HintType::StartingL2Output => {
                const OUTPUT_ROOT_VERSION: u8 = 0;
                const L2_TO_L1_MESSAGE_PASSER_ADDRESS: Address =
                    address!("4200000000000000000000000000000000000016");

                ensure!(hint.data.len() == 32, "Invalid hint data length");

                // Fetch the header for the L2 head block.
                let raw_header: Bytes = providers
                    .l2
                    .client()
                    .request("debug_getRawHeader", &[cfg.agreed_l2_head_hash])
                    .await?;
                let header = Header::decode(&mut raw_header.as_ref())?;

                // Fetch the storage root for the L2 head block.
                let l2_to_l1_message_passer = providers
                    .l2
                    .get_proof(L2_TO_L1_MESSAGE_PASSER_ADDRESS, Default::default())
                    .block_id(cfg.agreed_l2_head_hash.into())
                    .await?;

                let mut raw_output = [0u8; 128];
                raw_output[31] = OUTPUT_ROOT_VERSION;
                raw_output[32..64].copy_from_slice(header.state_root.as_ref());
                raw_output[64..96].copy_from_slice(l2_to_l1_message_passer.storage_hash.as_ref());
                raw_output[96..128].copy_from_slice(cfg.agreed_l2_head_hash.as_ref());
                let output_root = keccak256(raw_output);

                ensure!(
                    output_root == cfg.agreed_l2_output_root,
                    "Output root does not match L2 head."
                );

                let mut kv_write_lock = kv.write().await;
                kv_write_lock
                    .set(PreimageKey::new_keccak256(*output_root).into(), raw_output.into())?;
            }
            HintType::L2Code => {
                // geth hashdb scheme code hash key prefix
                const CODE_PREFIX: u8 = b'c';

                ensure!(hint.data.len() == 32, "Invalid hint data length");

                let hash: B256 = hint.data.as_ref().try_into()?;

                // Attempt to fetch the code from the L2 chain provider.
                let code_key = [&[CODE_PREFIX], hash.as_slice()].concat();
                let code = providers
                    .l2
                    .client()
                    .request::<&[Bytes; 1], Bytes>("debug_dbGet", &[code_key.into()])
                    .await;

                // Check if the first attempt to fetch the code failed. If it did, try fetching the
                // code hash preimage without the geth hashdb scheme prefix.
                let code = match code {
                    Ok(code) => code,
                    Err(_) => providers
                        .l2
                        .client()
                        .request::<&[B256; 1], Bytes>("debug_dbGet", &[hash])
                        .await
                        .map_err(|e| anyhow!("Error fetching code hash preimage: {e}"))?,
                };

                let mut kv_lock = kv.write().await;
                kv_lock.set(PreimageKey::new_keccak256(*hash).into(), code.into())?;
            }
            HintType::L2StateNode => {
                ensure!(hint.data.len() == 32, "Invalid hint data length");

                let hash: B256 = hint.data.as_ref().try_into()?;

                // Fetch the preimage from the L2 chain provider.
                let preimage: Bytes = providers.l2.client().request("debug_dbGet", &[hash]).await?;

                let mut kv_write_lock = kv.write().await;
                kv_write_lock.set(PreimageKey::new_keccak256(*hash).into(), preimage.into())?;
            }
            HintType::L2AccountProof => {
                ensure!(hint.data.len() == 8 + 20, "Invalid hint data length");

                let block_number = u64::from_be_bytes(hint.data.as_ref()[..8].try_into()?);
                let address = Address::from_slice(&hint.data.as_ref()[8..28]);

                let proof_response = providers
                    .l2
                    .get_proof(address, Default::default())
                    .block_id(block_number.into())
                    .await?;

                // Write the account proof nodes to the key-value store.
                let mut kv_lock = kv.write().await;
                proof_response.account_proof.into_iter().try_for_each(|node| {
                    let node_hash = keccak256(node.as_ref());
                    let key = PreimageKey::new_keccak256(*node_hash);
                    kv_lock.set(key.into(), node.into())?;
                    Ok::<(), anyhow::Error>(())
                })?;
            }
            HintType::L2AccountStorageProof => {
                ensure!(hint.data.len() == 8 + 20 + 32, "Invalid hint data length");

                let block_number = u64::from_be_bytes(hint.data.as_ref()[..8].try_into()?);
                let address = Address::from_slice(&hint.data.as_ref()[8..28]);
                let slot = B256::from_slice(&hint.data.as_ref()[28..]);

                let mut proof_response = providers
                    .l2
                    .get_proof(address, vec![slot])
                    .block_id(block_number.into())
                    .await?;

                let mut kv_lock = kv.write().await;

                // Write the account proof nodes to the key-value store.
                proof_response.account_proof.into_iter().try_for_each(|node| {
                    let node_hash = keccak256(node.as_ref());
                    let key = PreimageKey::new_keccak256(*node_hash);
                    kv_lock.set(key.into(), node.into())?;
                    Ok::<(), anyhow::Error>(())
                })?;

                // Write the storage proof nodes to the key-value store.
                let storage_proof = proof_response.storage_proof.remove(0);
                storage_proof.proof.into_iter().try_for_each(|node| {
                    let node_hash = keccak256(node.as_ref());
                    let key = PreimageKey::new_keccak256(*node_hash);
                    kv_lock.set(key.into(), node.into())?;
                    Ok::<(), anyhow::Error>(())
                })?;
            }
            HintType::L2PayloadWitness => {
                ensure!(hint.data.len() >= 32, "Invalid hint data length");

                let parent_block_hash = B256::from_slice(&hint.data.as_ref()[..32]);
                let payload_attributes: OpPayloadAttributes =
                    serde_json::from_slice(&hint.data[32..])?;

                let execute_payload_response: ExecutionWitness = providers
                    .l2
                    .client()
                    .request::<(B256, OpPayloadAttributes), ExecutionWitness>(
                        "debug_executePayload",
                        (parent_block_hash, payload_attributes),
                    )
                    .await
                    .map_err(|e| anyhow!("Failed to fetch preimage: {e}"))?;

                let mut merged = HashMap::<B256, Bytes>::default();
                merged.extend(execute_payload_response.state);
                merged.extend(execute_payload_response.codes);
                merged.extend(execute_payload_response.keys);

                let mut kv_lock = kv.write().await;
                for (hash, preimage) in merged.into_iter() {
                    let computed_hash = keccak256(preimage.as_ref());
                    assert_eq!(computed_hash, hash, "Preimage hash does not match expected hash");

                    let key = PreimageKey::new_keccak256(*hash);
                    kv_lock.set(key.into(), preimage.into())?;
                }
            }
            HintType::EigenDa => {
                ensure!(hint.data.len() > 32, "Invalid hint data length");

                let commitment = hint.data.to_vec();
                // Fetch the blob from the eigen da provider.
                let blob = providers
                    .eigen_da
                    .get_blob(&commitment)
                    .await
                    .map_err(|e| anyhow!("Failed to fetch blob: {e}"))?;
                let mut kv_lock = kv.write().await;

                // the fourth because 0x01010000 in the beginning is metadata
                let cert_blob_info = BlobInfo::decode(&mut &commitment[3..])
                    .map_err(|e| anyhow!("Failed to decode blob info: {e}"))?;
                // Proxy should return a cert whose data_length measured in symbol (i.e. 32 Bytes)
                let blob_length = cert_blob_info.blob_header.data_length as u64;

                let eigenda_blob = EigenDABlobData::encode(blob.as_ref());

                assert!(
                    eigenda_blob.blob.len() <= blob_length as usize * BYTES_PER_FIELD_ELEMENT,
                    "EigenDA blob size ({}) exceeds expected size ({})",
                    eigenda_blob.blob.len(),
                    blob_length as usize * BYTES_PER_FIELD_ELEMENT
                );

                //
                // Write all the field elements to the key-value store.
                // The preimage oracle key for each field element is the keccak256 hash of
                // `abi.encodePacked(cert.KZGCommitment, uint256(i))`

                //  TODO figure out the key size, most likely dependent on smart contract parsing
                let mut blob_key = [0u8; 96];
                blob_key[..32].copy_from_slice(cert_blob_info.blob_header.commitment.x.as_ref());
                blob_key[32..64].copy_from_slice(cert_blob_info.blob_header.commitment.y.as_ref());

                for i in 0..blob_length {
                    blob_key[88..].copy_from_slice(i.to_be_bytes().as_ref());
                    let blob_key_hash = keccak256(blob_key.as_ref());

                    kv_lock.set(
                        PreimageKey::new(*blob_key_hash, PreimageKeyType::Keccak256).into(),
                        blob_key.into(),
                    )?;
                    debug!("save block key, hash {:?}", blob_key_hash);
                    let start = (i as usize) << 5;
                    let end = start + 32;
                    let actual_end = eigenda_blob.blob.len().min(end);
                    let data_slice = if start >= eigenda_blob.blob.len() {
                        vec![0u8; 32]
                    } else {
                        let mut padded_data = vec![0u8; 32];
                        padded_data[..(actual_end - start)]
                            .copy_from_slice(&eigenda_blob.blob[start..actual_end]);
                        padded_data
                    };
                    kv_lock.set(
                        PreimageKey::new(*blob_key_hash, PreimageKeyType::GlobalGeneric).into(),
                        data_slice.into(),
                    )?;
                    debug!("save blob slice, hash {:?}", blob_key_hash);
                }

                // proof is at the random point
                //TODO
                // Because the blob_length in EigenDA is variable-length, KZG proofs cannot be cached at the position corresponding to blob_length
                // For now, they are placed at the position corresponding to commit x y. Further optimization will follow the EigenLayer approach
                let mut kzg_proof_key = [0u8; 64];
                kzg_proof_key[..64].copy_from_slice(blob_key[..64].as_ref());
                let kzg_proof_key_hash = keccak256(kzg_proof_key.as_ref());

                //TODO
                // In fact, the calculation result following the EigenLayer approach is not the same as the cert blob info.
                // need to save the real commitment x y
                let mut kzg_commitment_key = [0u8; 65];
                kzg_commitment_key[..64].copy_from_slice(blob_key[..64].as_ref());
                kzg_commitment_key[64] = 0u8;
                let kzg_commitment_key_hash = keccak256(kzg_commitment_key.as_ref());

                let mut witness = EigenDABlobWitness::new();

                let _ = witness
                    .push_witness(&blob)
                    .map_err(|e| anyhow!("eigen da blob push witness error {e}"))?;

                // let last_commitment = witness.commitments.last().unwrap();

                // make sure locally computed proof equals to returned proof from the provider
                // TODO In fact, the calculation result following the EigenLayer approach is not the same as the cert blob info.
                // if last_commitment[..32] != cert_blob_info.blob_header.commitment.x[..]
                //     || last_commitment[32..64] != cert_blob_info.blob_header.commitment.y[..]
                // {
                //     return Err(
                //         anyhow!("proxy commitment is different from computed commitment proxy",
                //     ));
                // };
                let proof: Vec<u8> =
                    witness.proofs.iter().flat_map(|x| x.as_ref().iter().copied()).collect();

                kv_lock.set(
                    PreimageKey::new(*kzg_proof_key_hash, PreimageKeyType::Keccak256).into(),
                    kzg_proof_key.into(),
                )?;
                debug!("save proof key, hash {:?}", kzg_proof_key_hash);
                // proof to be done
                kv_lock.set(
                    PreimageKey::new(*kzg_proof_key_hash, PreimageKeyType::GlobalGeneric).into(),
                    proof.into(),
                )?;
                debug!("save proof value, hash {:?}", kzg_proof_key_hash);

                let commitment: Vec<u8> =
                    witness.commitments.iter().flat_map(|x| x.as_ref().iter().copied()).collect();
                kv_lock.set(
                    PreimageKey::new(*kzg_commitment_key_hash, PreimageKeyType::Keccak256).into(),
                    kzg_commitment_key.into(),
                )?;
                debug!("save commitment key, hash {:?}", kzg_commitment_key_hash);

                // proof to be done
                kv_lock.set(
                    PreimageKey::new(*kzg_commitment_key_hash, PreimageKeyType::GlobalGeneric)
                        .into(),
                    commitment.into(),
                )?;
                debug!("save commitment value, hash {:?}", kzg_commitment_key_hash);
            }
        }

        Ok(())
    }
}
