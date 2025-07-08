#![doc = include_str!("../readme.md")]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/op-rs/kona/main/assets/square.png",
    html_favicon_url = "https://raw.githubusercontent.com/op-rs/kona/main/assets/favicon.ico",
    issue_tracker_base_url = "https://github.com/op-rs/kona/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod certificate;
mod constant;
mod eigenda_data;
mod utils;

pub use constant::BLOB_ENCODING_VERSION_0;
pub use constant::BYTES_PER_FIELD_ELEMENT;
pub use constant::STALE_GAP;

pub use eigenda_data::EigenDABlobData;

pub use certificate::BlobInfo;
pub use utils::*;
