/// Application-defined JSON-RPC codes for file (`-32030..-32039`).
///
/// 避开 identity `-32001..-32009`（跳过协议 `-32007`）、协议 `-32010`、
/// authn `-32020..-32029`。
///
/// `-32030` / `-32031` 曾用于 multipart 图片上传，已移除；勿复用直至客户端清干净。
pub mod codes {
    pub const ATTACHMENT_INVALID: i32 = -32032;
    pub const ATTACHMENT_NOT_FOUND: i32 = -32033;
    pub const ATTACHMENT_NOT_REGISTERED: i32 = -32034;
    pub const PRESIGN_UNSUPPORTED: i32 = -32035;
}

/// Domain-facing file / attachment errors.
///
/// [`Display`](std::fmt::Display) / [`l10n_key`](Self::l10n_key) is the Fluent key.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// commit：非 staging / 不安全 path。
    #[error("file_attachment_invalid")]
    AttachmentInvalid,
    /// commit：staging 对象不存在或 claim 缺失。
    #[error("file_attachment_not_found")]
    AttachmentNotFound,
    /// attach/detach：path 未在 blob_objects 登记。
    #[error("file_attachment_not_registered")]
    AttachmentNotRegistered,
    /// 预签名：当前后端无直传语义（如 test 内存后端 / 未配置本地 HMAC）。
    #[error("blob_presign_unsupported")]
    PresignUnsupported,
}

impl FileError {
    #[must_use]
    pub const fn rpc_code(&self) -> i32 {
        match self {
            Self::AttachmentInvalid => codes::ATTACHMENT_INVALID,
            Self::AttachmentNotFound => codes::ATTACHMENT_NOT_FOUND,
            Self::AttachmentNotRegistered => codes::ATTACHMENT_NOT_REGISTERED,
            Self::PresignUnsupported => codes::PRESIGN_UNSUPPORTED,
        }
    }

    #[must_use]
    pub const fn l10n_key(&self) -> &'static str {
        match self {
            Self::AttachmentInvalid => "file_attachment_invalid",
            Self::AttachmentNotFound => "file_attachment_not_found",
            Self::AttachmentNotRegistered => "file_attachment_not_registered",
            Self::PresignUnsupported => "blob_presign_unsupported",
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn l10n_key_matches_thiserror_display() {
        let cases = [
            FileError::AttachmentInvalid,
            FileError::AttachmentNotFound,
            FileError::AttachmentNotRegistered,
            FileError::PresignUnsupported,
        ];
        for err in cases {
            assert_eq!(err.l10n_key(), err.to_string());
        }
    }

    #[test]
    fn rpc_codes_stay_in_file_band() {
        for err in [
            FileError::AttachmentInvalid,
            FileError::AttachmentNotFound,
            FileError::AttachmentNotRegistered,
            FileError::PresignUnsupported,
        ] {
            let code = err.rpc_code();
            assert!((-32039..=-32030).contains(&code), "{code}");
            assert_ne!(code, -32007);
            assert_ne!(code, -32010);
        }
    }
}
