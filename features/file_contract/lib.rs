//! File / blob attachment contracts (toasty models + domain errors).
//!
//! Layering: other `features/*` may depend on this crate only — never on
//! `file`. This crate must not depend on any feature implementation.

mod error;
mod models;
mod path;

pub use error::{FileError, codes};
pub use models::blob_object::BlobObject;
pub use path::{STORAGE_DIR_PREFIX, staging_dated_file_path, staging_dir_date};
