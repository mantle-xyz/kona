// This file is @generated by prost-build.
/// CalldataFrame wraps the frame data or the eigenda blob reference to the frame data
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CalldataFrame {
    #[prost(oneof = "calldata_frame::Value", tags = "1, 2")]
    pub value: ::core::option::Option<calldata_frame::Value>,
}
/// Nested message and enum types in `CalldataFrame`.
pub mod calldata_frame {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Value {
        #[prost(bytes, tag = "1")]
        Frame(::prost::alloc::vec::Vec<u8>),
        #[prost(message, tag = "2")]
        FrameRef(super::FrameRef),
    }
}
/// This is a copy of BlobRequest here: <https://github.com/Layr-Labs/eigenda/blob/main/api/proto/retriever/retriever.proto#L10>
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FrameRef {
    /// defined by eigenda
    #[prost(bytes = "vec", tag = "1")]
    pub batch_header_hash: ::prost::alloc::vec::Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub blob_index: u32,
    #[prost(uint32, tag = "3")]
    pub reference_block_number: u32,
    #[prost(uint32, repeated, tag = "4")]
    pub quorum_ids: ::prost::alloc::vec::Vec<u32>,
    #[prost(uint32, tag = "5")]
    pub blob_length: u32,
    /// defined by mantle
    #[prost(bytes = "vec", tag = "100")]
    pub request_id: ::prost::alloc::vec::Vec<u8>,
    #[prost(bytes = "vec", tag = "101")]
    pub commitment: ::prost::alloc::vec::Vec<u8>,
}
