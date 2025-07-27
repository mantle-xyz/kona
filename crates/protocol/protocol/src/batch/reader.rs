//! Contains the [BatchReader] which is used to iteratively consume batches from raw data.

use crate::Batch;
use alloc::vec::Vec;
use alloy_primitives::Bytes;
use alloy_rlp::Decodable;
use kona_genesis::RollupConfig;
use miniz_oxide::inflate::decompress_to_vec_zlib;

/// Batch Reader provides a function that iteratively consumes batches from the reader.
/// The L1Inclusion block is also provided at creation time.
/// Warning: the batch reader can read every batch-type.
/// The caller of the batch-reader should filter the results.
#[derive(Debug)]
pub struct BatchReader {
    /// The raw data to decode.
    data: Option<Vec<u8>>,
    /// Decompressed data.
    decompressed: Vec<u8>,
    /// The current cursor in the `decompressed` data.
    cursor: usize,
    /// The maximum RLP bytes per channel.
    max_rlp_bytes_per_channel: usize,
}

impl BatchReader {
    /// Creates a new [BatchReader] from the given data and max decompressed RLP bytes per channel.
    pub fn new<T>(data: T, max_rlp_bytes_per_channel: usize) -> Self
    where
        T: Into<Vec<u8>>,
    {
        Self {
            data: Some(data.into()),
            decompressed: Vec::new(),
            cursor: 0,
            max_rlp_bytes_per_channel,
        }
    }

    /// Pulls out the next batch from the reader.
    pub fn next_batch(&mut self, cfg: &RollupConfig) -> Option<Batch> {
        if let Some(data) = self.data.take() {
            // Peek at the data to determine the compression type.
            if data.is_empty() {
                return None;
            }

            self.decompressed = decompress_to_vec_zlib(&data).ok()?;

            // Check the size of the decompressed channel RLP.
            if self.decompressed.len() > self.max_rlp_bytes_per_channel {
                return None;
            }
        }

        // Decompress and RLP decode the batch data, before finally decoding the batch itself.
        let decompressed_reader = &mut self.decompressed.as_slice()[self.cursor..].as_ref();
        let bytes = Bytes::decode(decompressed_reader).ok()?;
        let result = Batch::decode(&mut bytes.as_ref(), cfg);
        match result {
            Ok(batch) => {
                // Advance the cursor on the reader.
                self.cursor = self.decompressed.len() - decompressed_reader.len();
                Some(batch)
            }
            Err(_) => None,
        }

        // let Ok(batch) = Batch::decode(&mut bytes.as_ref(), cfg) else {
        //     error!(target: "batch-reader", "Failed to decode batch, skipping batch");
        //     return None;
        // };
        //
        // // Advance the cursor on the reader.
        // self.cursor = self.decompressed.len() - decompressed_reader.len();
        // Some(batch)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use kona_genesis::{
        HardForkConfig, MAX_RLP_BYTES_PER_CHANNEL_BEDROCK, MAX_RLP_BYTES_PER_CHANNEL_FJORD,
    };

    fn new_compressed_batch_data() -> Bytes {
        let file_contents =
            alloc::string::String::from_utf8_lossy(include_bytes!("../../testdata/batch.hex"));
        let file_contents = &(&*file_contents)[..file_contents.len() - 1];
        let data = alloy_primitives::hex::decode(file_contents).unwrap();
        data.into()
    }

    #[test]
    fn test_batch_reader() {
        let raw = new_compressed_batch_data();
        let decompressed_len = decompress_to_vec_zlib(&raw).unwrap().len();
        let mut reader = BatchReader::new(raw, MAX_RLP_BYTES_PER_CHANNEL_BEDROCK as usize);
        reader.next_batch(&RollupConfig::default()).unwrap();
        assert_eq!(reader.cursor, decompressed_len);
    }

    #[test]
    fn test_batch_reader_fjord() {
        let raw = new_compressed_batch_data();
        let decompressed_len = decompress_to_vec_zlib(&raw).unwrap().len();
        let mut reader = BatchReader::new(raw, MAX_RLP_BYTES_PER_CHANNEL_FJORD as usize);
        reader
            .next_batch(&RollupConfig {
                hardforks: HardForkConfig { fjord_time: Some(0), ..Default::default() },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(reader.cursor, decompressed_len);
    }
}
