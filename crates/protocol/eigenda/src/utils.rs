use crate::certificate::BlobInfo;
use alloy_primitives::keccak256;
use alloy_rlp::Decodable;
use kona_derive::errors::BlobDecodingError;

/// EigenDA blob processing constants
pub const METADATA_SIZE: usize = 3;
/// Minimum header size for EigenDA blobs
pub const MIN_HEADER_SIZE: usize = 32;
/// Size of the blob key in bytes
pub const BLOB_KEY_SIZE: usize = 96;
/// Size of a field element in bytes
pub const FIELD_ELEMENT_SIZE: usize = 32;
/// Size of the KZG proof key in bytes
pub const KZG_PROOF_KEY_SIZE: usize = 64;
/// Size of the KZG commitment key in bytes
pub const KZG_COMMITMENT_KEY_SIZE: usize = 65;
/// Size of commitment coordinates in bytes
pub const COMMITMENT_COORD_SIZE: usize = 32;

/// Blob key offsets
pub const COMMITMENT_X_OFFSET: usize = 0;
/// Offset for commitment Y coordinate
pub const COMMITMENT_Y_OFFSET: usize = 32;
/// Offset for field index in blob key
pub const FIELD_INDEX_OFFSET: usize = 88;

/// Validates commitment contains header + metadata
pub const fn validate_commitment_length(commitment: &[u8]) -> Result<(), BlobDecodingError> {
    if commitment.len() <= MIN_HEADER_SIZE + METADATA_SIZE {
        return Err(BlobDecodingError::InvalidLength);
    }
    Ok(())
}

/// Decodes BlobInfo from a commitment, skipping the metadata prefix
pub fn decode_blob_info_from_commitment(commitment: &[u8]) -> Result<BlobInfo, BlobDecodingError> {
    validate_commitment_length(commitment)?;

    // Skip first 3 bytes: metadata like cert version, OP generic commitment
    // See: https://github.com/Layr-Labs/eigenda-proxy/blob/main/commitments/mode.go#L39
    BlobInfo::decode(&mut &commitment[METADATA_SIZE..])
        .map_err(|_| BlobDecodingError::InvalidLength)
}

/// Creates a blob key template for field element retrieval
/// Key structure: [commitment_x (32 bytes)] + [commitment_y (32 bytes)] + [padding] + [field_index (8 bytes)]
pub fn create_blob_key_template(blob_info: &BlobInfo) -> [u8; BLOB_KEY_SIZE] {
    let mut blob_key = [0u8; BLOB_KEY_SIZE];
    blob_key[COMMITMENT_X_OFFSET..COMMITMENT_X_OFFSET + FIELD_ELEMENT_SIZE]
        .copy_from_slice(&blob_info.blob_header.commitment.x);
    blob_key[COMMITMENT_Y_OFFSET..COMMITMENT_Y_OFFSET + FIELD_ELEMENT_SIZE]
        .copy_from_slice(&blob_info.blob_header.commitment.y);
    blob_key
}

/// Updates blob key with field index
pub fn update_blob_key_with_index(blob_key: &mut [u8; BLOB_KEY_SIZE], field_index: u64) {
    blob_key[FIELD_INDEX_OFFSET..].copy_from_slice(&field_index.to_be_bytes());
}

/// Calculates the keccak256 hash of the blob key
pub fn calculate_blob_key_hash(blob_key: &[u8; BLOB_KEY_SIZE]) -> [u8; 32] {
    keccak256(blob_key).into()
}

/// Extracts and pads field element data from blob
pub fn extract_field_element(
    blob_data: &[u8],
    field_index: u64,
    field_element_size: usize,
) -> Vec<u8> {
    let field_start = (field_index as usize) * field_element_size;
    let field_end = field_start + field_element_size;
    let available_end = blob_data.len().min(field_end);

    if field_start >= blob_data.len() {
        // Field is beyond blob data - return zero-padded field
        vec![0u8; field_element_size]
    } else {
        // Extract available data and pad to field size
        let mut padded_field = vec![0u8; field_element_size];
        let copy_len = available_end - field_start;
        padded_field[..copy_len].copy_from_slice(&blob_data[field_start..available_end]);
        padded_field
    }
}

/// Creates a KZG proof key from blob key
pub fn create_kzg_proof_key(blob_key: &[u8; BLOB_KEY_SIZE]) -> [u8; KZG_PROOF_KEY_SIZE] {
    let mut kzg_proof_key = [0u8; KZG_PROOF_KEY_SIZE];
    kzg_proof_key.copy_from_slice(&blob_key[..KZG_PROOF_KEY_SIZE]);
    kzg_proof_key
}

/// Creates a KZG commitment key from blob key
pub fn create_kzg_commitment_key(blob_key: &[u8; BLOB_KEY_SIZE]) -> [u8; KZG_COMMITMENT_KEY_SIZE] {
    let mut kzg_commitment_key = [0u8; KZG_COMMITMENT_KEY_SIZE];
    kzg_commitment_key[..KZG_PROOF_KEY_SIZE].copy_from_slice(&blob_key[..KZG_PROOF_KEY_SIZE]);
    kzg_commitment_key[KZG_PROOF_KEY_SIZE] = 0u8;
    kzg_commitment_key
}

/// Calculates the blob size in bytes from field element count
pub const fn calculate_blob_size_bytes(field_element_count: u64) -> usize {
    field_element_count as usize * crate::BYTES_PER_FIELD_ELEMENT
}

/// Validates blob size against expected field count
pub const fn validate_blob_size(
    blob_size: usize,
    expected_field_count: u64,
) -> Result<(), BlobDecodingError> {
    let expected_max_size = expected_field_count as usize * crate::BYTES_PER_FIELD_ELEMENT;
    if blob_size > expected_max_size {
        return Err(BlobDecodingError::InvalidLength);
    }
    Ok(())
}
