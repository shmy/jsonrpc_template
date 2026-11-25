//! File / blob attachment contracts (toasty models + domain errors).
//!
//! Layering: `biz/*` / future `ops/*` may depend on this crate only — never on
//! `file`. This crate must not depend on any surface implementation.

mod error;
mod models;
mod path;

pub use error::{FileError, codes};
pub use models::blob_object::BlobObject;
pub use path::{
    DEFAULT_DERIVE_SIZE, DERIVE_EXTENSION, DERIVE_MARK, DERIVE_SIZES, DERIVE_SIZES_HINT,
    DeriveRequest, LEGACY_DERIVE_MARK, LEGACY_THUMB_SUFFIX, STORAGE_DIR_PREFIX,
    derived_cache_paths, derived_object_path, is_allowed_derive_size, is_image_extension,
    resolve_derive_request, staging_dated_file_path, staging_dir_date, thumb_object_path,
};
