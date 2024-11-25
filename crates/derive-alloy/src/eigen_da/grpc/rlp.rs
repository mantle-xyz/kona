use Vec;
use bytes::{Bytes, BytesMut};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use crate::eigen_da::common::G1Commitment;
use crate::eigen_da::{BlobInfo, BatchHeader, BatchMetadata, BlobHeader, BlobQuorumParam, BlobVerificationProof};

impl Encodable for BlobInfo {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2);
        match &self.blob_header {
            Some(blob_header) => {s.append(blob_header);},
            None => {s.append_empty_data();},
        }
        match &self.blob_verification_proof {
            Some(blob_verification_proof) => {s.append(blob_verification_proof);},
            None => {s.append_empty_data();},
        }
        ;
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for BlobInfo {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let blob_header = if !rlp.at(0)?.is_empty() {
            Some(rlp.val_at(0)?)
        } else { None };
        let blob_verification_proof = if !rlp.at(1)?.is_empty() {
            Some(rlp.val_at(1)?)
        }else { None };
        Ok( BlobInfo{
            blob_header,
            blob_verification_proof,
        })
    }
}




impl Encodable for BlobVerificationProof {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(5);
        s.append(&self.batch_id);
        s.append(&self.blob_index);
        match &self.batch_metadata {
            Some(batch_metadata) => {s.append(batch_metadata);},
            None => {s.append_empty_data();},
        }
        s.append(&self.inclusion_proof);
        s.append(&self.quorum_indexes);
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for BlobVerificationProof {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 5 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let batch_id = rlp.val_at(0)?;
        let blob_index = rlp.val_at(1)?;
        let batch_metadata = if !rlp.at(2)?.is_empty() {
            Some(rlp.val_at(2)?)
        } else { None };
        let inclusion_proof = rlp.val_at(3)?;
        let quorum_indexes = rlp.list_at(4)?;
        Ok(BlobVerificationProof{
            batch_id,
            blob_index,
            batch_metadata,
            inclusion_proof,
            quorum_indexes,
        })
    }
}

impl Encodable for BatchMetadata {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(5);
        match &self.batch_header {
            Some(batch_header) => {s.append(batch_header);},
            None => {s.append_empty_data();},
        }
        s.append(&self.signatory_record_hash);
        s.append(&self.fee);
        s.append(&self.confirmation_block_number);
        s.append(&self.batch_header_hash);
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for BatchMetadata {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 5 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let batch_header = if !rlp.at(0)?.is_empty() {
            Some(rlp.at(0)?.as_val()?)
        } else { None };
        let signatory_record_hash = rlp.at(1)?.as_val()?;
        let fee = rlp.at(2)?.as_val()?;
        let confirmation_block_number = rlp.at(3)?.as_val()?;
        let batch_header_hash = rlp.at(4)?.as_val()?;
        Ok( BatchMetadata{
            batch_header,
            signatory_record_hash,
            fee,
            confirmation_block_number,
            batch_header_hash,
        })
    }
}

impl Encodable for BatchHeader {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(4);
        s.append(&self.batch_root);
        s.append(&self.quorum_numbers);
        s.append(&self.quorum_signed_percentages);
        s.append(&self.reference_block_number);
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for BatchHeader {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 4 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let batch_root = rlp.val_at(0)?;
        let quorum_numbers = rlp.val_at(1)?;
        let quorum_signed_percentages = rlp.val_at(2)?;
        let reference_block_number = rlp.val_at(3)?;
        Ok( BatchHeader {
            batch_root,
            quorum_numbers,
            quorum_signed_percentages,
            reference_block_number,
        })
    }
}






impl Encodable for BlobHeader {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(3);
        match &self.commitment {
            Some(commitment) => {s.append(commitment);},
            None => {s.append_empty_data();},
        }
        s.append(&self.data_length);
        s.begin_list(self.blob_quorum_params.len());
        for param in &self.blob_quorum_params {
            s.append(param);
        }
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for BlobHeader {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? < 3 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let commitment = if !rlp.at(0)?.is_empty() {
            Some(rlp.at(0)?.as_val()?)
        } else { None };

        let data_length = rlp.val_at(1)?;
        let blob_quorum_params = rlp.list_at(2)?;
        Ok( BlobHeader { commitment, blob_quorum_params, data_length } )
    }
}



impl Encodable for G1Commitment {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2);
        s.append(&self.x);
        s.append(&self.y);
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for G1Commitment {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let x = rlp.val_at(0)?;
        let y = rlp.val_at(1)?;

        Ok(G1Commitment {
            x,
            y,
        })
    }
}

impl Encodable for BlobQuorumParam {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(4);
        s.append(&self.quorum_number);
        s.append(&self.adversary_threshold_percentage);
        s.append(&self.confirmation_threshold_percentage);
        s.append(&self.chunk_length);
    }
}

#[allow(elided_lifetimes_in_paths)]
impl Decodable for BlobQuorumParam {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 4 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let quorum_number = rlp.val_at(0)?;

        let adversary_threshold_percentage = rlp.val_at(1)?;
        let confirmation_threshold_percentage = rlp.val_at(2)?;
        let chunk_length = rlp.val_at(3)?;
        Ok(BlobQuorumParam {
            quorum_number,
            adversary_threshold_percentage,
            confirmation_threshold_percentage,
            chunk_length,
        })
    }
}


