use alloc::vec;
use alloc::vec::Vec;

const BYTES_PER_SYMBOL: usize = 32;

/// ConvertByPaddingEmptyByte takes bytes and insert an empty byte at the front of every 31 byte.
/// The empty byte is padded at the low address, because we use big endian to interpret a fiedl element.
/// This ensure every 32 bytes are within the valid range of a field element for bn254 curve.
/// If the input data is not a multiple of 31, the reminder is added to the output by
/// inserting a 0 and the reminder. The output does not necessarily be a multipler of 32
pub(crate) fn convert_by_padding_empty_byte(data: &[u8]) -> Vec<u8> {
    let data_size = data.len();
    let parse_size = BYTES_PER_SYMBOL - 1;
    let put_size = BYTES_PER_SYMBOL;

    // calculate the total len
    let data_len = (data_size + parse_size - 1) / parse_size;
    let mut valid_data = vec![0u8; data_len * put_size];
    let mut valid_end = valid_data.len();

    for i in 0..data_len {
        let start = i * parse_size;
        let mut end = (i + 1) * parse_size;
        if end > data.len() {
            end = data.len();
            valid_end = end - start + 1 + i * put_size;
        }

        // 填充前导字节
        valid_data[i * BYTES_PER_SYMBOL] = 0x00;
        valid_data[i * BYTES_PER_SYMBOL + 1..(i + 1) * BYTES_PER_SYMBOL]
            .copy_from_slice(&data[start..end]);
    }

    valid_data.truncate(valid_end);
    valid_data
}


/// RemoveEmptyByteFromPaddedBytes takes bytes and remove the first byte from every 32 bytes.
/// This reverses the change made by the function ConvertByPaddingEmptyByte.
/// The function does not assume the input is a multiple of BYTES_PER_SYMBOL(32 bytes).
/// For the reminder of the input, the first byte is taken out, and the rest is appended to
/// the output.
pub(crate) fn remove_empty_byte_from_padded_bytes(data: &[u8]) -> Vec<u8> {
    let data_size = data.len();
    let parse_size = BYTES_PER_SYMBOL;
    let data_len = (data_size + parse_size - 1) / parse_size;

    let put_size = BYTES_PER_SYMBOL - 1;
    let mut valid_data = vec![0u8; data_len * put_size];
    let mut valid_len = valid_data.len();

    for i in 0..data_len {
        let start = i * parse_size + 1;
        let mut end = (i + 1) * parse_size;

        if end > data.len() {
            end = data.len();
            valid_len = end - start + i * put_size;
        }

        valid_data[i * put_size..(i + 1) * put_size]
            .copy_from_slice(&data[start..end]);
    }

    valid_data.truncate(valid_len);
    valid_data
}