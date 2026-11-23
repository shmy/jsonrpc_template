//! File / blob attachment contracts (toasty models + domain errors).
//!
//! Layering: other `features/*` may depend on this crate only — never on
//! `file`. This crate must not depend on any feature implementation.

mod error;
mod models;
mod path;

pub use error::{FileError, codes};
pub use models::blob_object::BlobObject;
pub use path::{
    DEFAULT_DERIVE_WIDTH, DERIVE_EXTENSION, DERIVE_WIDTHS, DERIVE_WIDTHS_HINT, DeriveRequest,
    LEGACY_THUMB_SUFFIX, STORAGE_DIR_PREFIX, derived_cache_paths, derived_object_path,
    is_allowed_derive_width, is_image_extension, resolve_derive_request, staging_dated_file_path,
    staging_dir_date, thumb_object_path,
};
