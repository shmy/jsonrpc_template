use crate::readable_duration::ReadableDuration;
use clap::Parser;
pub use secrecy::ExposeSecret;
use secrecy::SecretString;

const DEFAULT_SEED_ADMIN_PASSWORD: &str = "admin123!";

#[derive(Clone, Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub log: LogCli,

    #[command(flatten)]
    pub server: ServerCli,

    #[command(flatten)]
    pub otlp: OtlpCli,

    #[command(flatten)]
    pub database: DatabaseCli,

    #[command(flatten)]
    pub jwt: JwtCli,

    #[command(flatten)]
    pub cache: CacheCli,

    #[command(flatten)]
    pub blob: BlobCli,

    #[command(flatten)]
    pub rate_limit: RateLimitCli,

    #[command(flatten)]
    pub seed: SeedCli,
}

#[derive(Clone, Debug, clap::Args)]
pub struct LogCli {
    #[arg(
        id = "log_level",
        long = "log-level",
        env = "RUST_LOG",
        default_value = "info"
    )]
    pub level: String,
}

#[derive(Clone, Debug, clap::Args)]
pub struct ServerCli {
    #[arg(
        id = "listen_addr",
        long = "listen-addr",
        default_value = "0.0.0.0:8080",
        env = "LISTEN_ADDR"
    )]
    pub listen_addr: String,

    /// 仅 master 做 migrate / seed / blob GC；多实例只开一个 `MASTER=true`。
    #[arg(
        id = "master",
        long = "master",
        default_value = "false",
        env = "MASTER"
    )]
    pub master: bool,

    /// Whether to mount the `GET /docs` debug page (`DOCS_ENABLED=true` to enable).
    #[arg(
        id = "docs_enabled",
        long = "docs-enabled",
        default_value = "false",
        env = "DOCS_ENABLED"
    )]
    pub docs_enabled: bool,

    #[arg(
        id = "request_timeout",
        long = "request-timeout",
        default_value = "60s",
        env = "REQUEST_TIMEOUT"
    )]
    pub request_timeout: ReadableDuration,

    #[arg(
        id = "body_limit_bytes",
        long = "body-limit-bytes",
        default_value_t = 1024 * 1024,
        env = "BODY_LIMIT_BYTES"
    )]
    pub body_limit_bytes: usize,

    #[arg(
        id = "max_batch_len",
        long = "max-batch-len",
        default_value_t = 128,
        env = "MAX_BATCH_LEN"
    )]
    pub max_batch_len: usize,

    /// Comma-separated Origin whitelist. Empty = no CORS layer.
    #[arg(
        id = "cors_origins",
        long = "cors-origins",
        default_value = "",
        env = "CORS_ORIGINS"
    )]
    pub cors_origins: String,

    /// Comma-separated trusted proxy IPs/CIDRs for `X-Forwarded-For` parsing.
    #[arg(
        id = "trusted_proxies",
        long = "trusted-proxies",
        default_value = "",
        env = "TRUSTED_PROXIES"
    )]
    pub trusted_proxies: String,
}

#[derive(Clone, Debug, clap::Args)]
pub struct RateLimitCli {
    /// login/refresh Redis IP sliding-window rate (hits per second baseline).
    /// 全站限流交给网关；应用内仅贵路径 method 调用 `AuthRateLimit::allow`。
    #[arg(
        id = "rate_limit_auth_per_second",
        long = "rate-limit-auth-per-second",
        default_value_t = 5,
        env = "RATE_LIMIT_AUTH_PER_SECOND"
    )]
    pub auth_per_second: u32,

    /// login/refresh Redis IP sliding-window burst (max hits in the derived window).
    #[arg(
        id = "rate_limit_auth_burst",
        long = "rate-limit-auth-burst",
        default_value_t = 20,
        env = "RATE_LIMIT_AUTH_BURST"
    )]
    pub auth_burst: u32,

    /// login 按手机号固定窗时长（成功也计数；防单号撞库）。
    #[arg(
        id = "rate_limit_login_phone_window",
        long = "rate-limit-login-phone-window",
        default_value = "5m",
        env = "RATE_LIMIT_LOGIN_PHONE_WINDOW"
    )]
    pub login_phone_window: ReadableDuration,

    /// login 按手机号固定窗内最大尝试次数。
    #[arg(
        id = "rate_limit_login_phone_max_attempts",
        long = "rate-limit-login-phone-max-attempts",
        default_value_t = 10,
        env = "RATE_LIMIT_LOGIN_PHONE_MAX_ATTEMPTS"
    )]
    pub login_phone_max_attempts: usize,
}

#[derive(Clone, Debug, clap::Args)]
pub struct OtlpCli {
    /// Collector base URL。空字符串则关闭 OTLP 三信号导出（仅 console）。
    #[arg(
        id = "otlp_endpoint",
        long = "otlp-endpoint",
        env = "OTLP_ENDPOINT",
        default_value = "http://localhost:4317"
    )]
    pub endpoint: String,

    #[arg(
        id = "otlp_service_name",
        long = "otlp-service-name",
        env = "OTLP_SERVICE_NAME",
        default_value = "jsonrpc-template"
    )]
    pub service_name: String,

    #[arg(
        id = "otlp_metadata",
        long = "otlp-metadata",
        env = "OTLP_METADATA",
        default_value = "{}",
        value_parser = parse_secret_string
    )]
    pub metadata: SecretString,

    /// Trace 采样率 `(0, 1]`；`1` = 全量。parent-based TraceIdRatio。
    #[arg(
        id = "otlp_sampler_ratio",
        long = "otlp-sampler-ratio",
        env = "OTLP_SAMPLER_RATIO",
        default_value_t = 0.1
    )]
    pub sampler_ratio: f64,
}

#[derive(Clone, Debug, clap::Args)]
pub struct SeedCli {
    #[arg(
        id = "seed_admin_phone",
        long = "seed-admin-phone",
        env = "SEED_ADMIN_PHONE",
        default_value = "13888888888"
    )]
    pub admin_phone: String,

    #[arg(
        id = "seed_admin_password",
        long = "seed-admin-password",
        env = "SEED_ADMIN_PASSWORD",
        default_value = DEFAULT_SEED_ADMIN_PASSWORD,
        value_parser = parse_secret_string
    )]
    pub admin_password: SecretString,
}

#[derive(Clone, Debug, clap::Args)]
pub struct DatabaseCli {
    #[arg(
        id = "database_url",
        long = "database-url",
        env = "DATABASE_URL",
        value_parser = parse_secret_string
    )]
    pub url: SecretString,

    /// 当前 toasty 只暴露 max_pool_size；不声明未生效的池参数。
    #[arg(
        id = "database_max_connections",
        long = "database-max-connections",
        default_value = "20",
        env = "DATABASE_MAX_CONNECTIONS"
    )]
    pub max_connections: u32,
}

#[derive(Clone, Debug, clap::Args)]
pub struct JwtCli {
    #[arg(
        id = "jwt_secret",
        long = "jwt-secret",
        env = "JWT_SECRET",
        value_parser = parse_jwt_secret
    )]
    pub secret: SecretString,

    #[arg(
        id = "access_token_ttl",
        long = "access-token-ttl",
        default_value = "15min",
        env = "ACCESS_TOKEN_TTL"
    )]
    pub access_ttl: ReadableDuration,

    #[arg(
        id = "refresh_token_ttl",
        long = "refresh-token-ttl",
        default_value = "30d",
        env = "REFRESH_TOKEN_TTL"
    )]
    pub refresh_ttl: ReadableDuration,

    /// 旧 refresh 重放宽限；`0` = 关闭（命中 used 即吊销）。客户端超时重试可设 `30s`。
    #[arg(
        id = "refresh_grace_ttl",
        long = "refresh-grace-ttl",
        default_value = "30s",
        env = "REFRESH_GRACE_TTL"
    )]
    pub refresh_grace_ttl: ReadableDuration,

    #[arg(
        id = "jwt_issuer",
        long = "jwt-issuer",
        env = "JWT_ISSUER",
        default_value = "jsonrpc-template"
    )]
    pub issuer: String,

    #[arg(
        id = "jwt_audience",
        long = "jwt-audience",
        env = "JWT_AUDIENCE",
        default_value = "account"
    )]
    pub audience: String,
}

#[derive(Clone, Debug, clap::Args)]
pub struct CacheCli {
    #[arg(
        id = "cache_redis_url",
        long = "cache-redis-url",
        default_value = "redis://127.0.0.1:6379",
        env = "CACHE_REDIS_URL"
    )]
    pub cache_redis_url: String,
}

#[derive(Clone, Debug, clap::Args)]
pub struct BlobCli {
    /// 本地落盘根目录（feature `blob-fs` / `config/fs`；默认 `data/blob`）。
    #[cfg(feature = "fs")]
    #[arg(
        id = "blob_fs_root",
        long = "blob-fs-root",
        env = "BLOB_FS_ROOT",
        default_value = "data/blob",
        value_parser = parse_nonempty_string
    )]
    pub blob_fs_root: String,

    /// 本地公网前缀，须与实际对外 URL 一致（feature `blob-fs` **必填**）。
    #[cfg(feature = "fs")]
    #[arg(
        id = "blob_fs_domain",
        long = "blob-fs-domain",
        env = "BLOB_FS_DOMAIN",
        value_parser = parse_nonempty_string
    )]
    pub blob_fs_domain: String,

    /// Fs 本地预签名 HMAC 密钥（≥32 字节；**勿**复用 `JWT_SECRET`；feature `blob-fs` 必填）。
    #[cfg(feature = "fs")]
    #[arg(
        id = "blob_fs_presign_secret",
        long = "blob-fs-presign-secret",
        env = "BLOB_FS_PRESIGN_SECRET",
        value_parser = parse_blob_fs_presign_secret
    )]
    pub blob_fs_presign_secret: SecretString,

    /// `file.presign` 允许的扩展名（逗号分隔、无点）；空 = 拒绝全部。
    #[arg(
        id = "blob_allowed_extensions",
        long = "blob-allowed-extensions",
        env = "BLOB_ALLOWED_EXTENSIONS",
        default_value = "webp,jpg,jpeg,png,gif,mp4,webm",
        value_delimiter = ','
    )]
    pub allowed_extensions: Vec<String>,

    /// COS 公网访问域名（feature `blob-cos` / `config/cos` 必填）。
    #[cfg(feature = "cos")]
    #[arg(
        id = "blob_s3_domain",
        long = "blob-s3-domain",
        env = "BLOB_S3_DOMAIN",
        value_parser = parse_nonempty_string
    )]
    pub blob_s3_domain: String,

    /// COS API endpoint（feature `blob-cos` 必填）。
    #[cfg(feature = "cos")]
    #[arg(
        id = "blob_s3_endpoint",
        long = "blob-s3-endpoint",
        env = "BLOB_S3_ENDPOINT",
        value_parser = parse_nonempty_string
    )]
    pub blob_s3_endpoint: String,

    /// COS bucket（feature `blob-cos` 必填）。
    #[cfg(feature = "cos")]
    #[arg(
        id = "blob_s3_bucket",
        long = "blob-s3-bucket",
        env = "BLOB_S3_BUCKET",
        value_parser = parse_nonempty_string
    )]
    pub blob_s3_bucket: String,

    /// COS SecretId（feature `blob-cos` 必填）。
    #[cfg(feature = "cos")]
    #[arg(
        id = "blob_s3_secret_id",
        long = "blob-s3-secret-id",
        env = "BLOB_S3_SECRET_ID",
        value_parser = parse_nonempty_secret
    )]
    pub blob_s3_secret_id: SecretString,

    /// COS SecretKey（feature `blob-cos` 必填）。
    #[cfg(feature = "cos")]
    #[arg(
        id = "blob_s3_secret_key",
        long = "blob-s3-secret-key",
        env = "BLOB_S3_SECRET_KEY",
        value_parser = parse_nonempty_secret
    )]
    pub blob_s3_secret_key: SecretString,
}

#[cfg(any(feature = "fs", feature = "cos"))]
fn parse_nonempty_string(s: &str) -> Result<String, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("must not be empty".to_string());
    }
    Ok(trimmed.to_owned())
}

fn parse_secret_string(s: &str) -> Result<SecretString, String> {
    Ok(SecretString::from(s))
}

#[cfg(feature = "cos")]
fn parse_nonempty_secret(s: &str) -> Result<SecretString, String> {
    parse_nonempty_string(s).map(SecretString::from)
}

const MIN_JWT_SECRET_LEN: usize = 32;
#[cfg(feature = "fs")]
const MIN_BLOB_FS_PRESIGN_SECRET_LEN: usize = 32;

fn parse_jwt_secret(s: &str) -> Result<SecretString, String> {
    if s.len() < MIN_JWT_SECRET_LEN {
        return Err(format!(
            "JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} bytes"
        ));
    }
    Ok(SecretString::from(s))
}

#[cfg(feature = "fs")]
fn parse_blob_fs_presign_secret(s: &str) -> Result<SecretString, String> {
    if s.len() < MIN_BLOB_FS_PRESIGN_SECRET_LEN {
        return Err(format!(
            "BLOB_FS_PRESIGN_SECRET must be at least {MIN_BLOB_FS_PRESIGN_SECRET_LEN} bytes"
        ));
    }
    Ok(SecretString::from(s))
}

impl Cli {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.otlp.sampler_ratio > 0.0 && self.otlp.sampler_ratio <= 1.0) {
            return Err(
                "OTLP_SAMPLER_RATIO must be in (0, 1] (use empty OTLP_ENDPOINT to disable OTLP)"
                    .to_string(),
            );
        }
        if self.server.body_limit_bytes == 0 {
            return Err("BODY_LIMIT_BYTES must be > 0".to_string());
        }
        if self.server.max_batch_len == 0 {
            return Err("MAX_BATCH_LEN must be > 0".to_string());
        }
        for (name, value) in [
            (
                "RATE_LIMIT_AUTH_PER_SECOND",
                self.rate_limit.auth_per_second,
            ),
            ("RATE_LIMIT_AUTH_BURST", self.rate_limit.auth_burst),
        ] {
            if value == 0 {
                return Err(format!("{name} must be > 0"));
            }
        }
        if self.rate_limit.login_phone_window.duration().is_zero() {
            return Err("RATE_LIMIT_LOGIN_PHONE_WINDOW must be > 0".to_string());
        }
        if self.rate_limit.login_phone_max_attempts == 0 {
            return Err("RATE_LIMIT_LOGIN_PHONE_MAX_ATTEMPTS must be > 0".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::readable_duration;
    use clap::Parser;
    use std::env::remove_var;
    use std::env::set_var;

    fn base_args() -> Vec<&'static str> {
        #[allow(unused_mut)] // mutated only when `fs` / `cos` features enable blob args
        let mut args = vec![
            "api",
            "--database-url",
            "postgres://localhost/test",
            "--jwt-secret",
            "test-jwt-secret-at-least-32-bytes!!",
        ];
        #[cfg(feature = "fs")]
        {
            args.extend([
                "--blob-fs-root",
                "data/blob",
                "--blob-fs-domain",
                "http://127.0.0.1:8080/files",
                "--blob-fs-presign-secret",
                "test-blob-presign-secret-at-least-32b",
            ]);
        }
        #[cfg(feature = "cos")]
        {
            args.extend([
                "--blob-s3-domain",
                "https://bucket.cos.example.com",
                "--blob-s3-endpoint",
                "https://cos.example.com",
                "--blob-s3-bucket",
                "bucket",
                "--blob-s3-secret-id",
                "secret-id",
                "--blob-s3-secret-key",
                "secret-key",
            ]);
        }
        args
    }

    fn base_cli() -> Cli {
        Cli::try_parse_from(base_args()).expect("parse")
    }

    fn set_arg_value(args: &mut [&'static str], flag: &str, value: &'static str) {
        let idx = args.iter().position(|a| *a == flag).expect("flag present") + 1;
        *args.get_mut(idx).expect("flag value slot") = value;
    }

    #[test]
    fn jwt_secret_must_be_long_enough() {
        let mut args = base_args();
        set_arg_value(&mut args, "--jwt-secret", "too-short");
        let err = Cli::try_parse_from(args).unwrap_err();
        assert!(err.to_string().contains("JWT_SECRET"));
    }

    #[cfg(feature = "fs")]
    #[test]
    fn blob_fs_presign_secret_must_be_long_enough() {
        let mut args = base_args();
        set_arg_value(&mut args, "--blob-fs-presign-secret", "too-short");
        let err = Cli::try_parse_from(args).unwrap_err();
        assert!(err.to_string().contains("BLOB_FS_PRESIGN_SECRET"));
    }

    #[cfg(feature = "fs")]
    #[test]
    fn blob_fs_vars_required_when_fs_feature() {
        let err = Cli::try_parse_from([
            "api",
            "--database-url",
            "postgres://localhost/test",
            "--jwt-secret",
            "test-jwt-secret-at-least-32-bytes!!",
        ])
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("blob-fs-root")
                || msg.contains("BLOB_FS_ROOT")
                || msg.contains("required"),
            "{msg}"
        );
    }

    #[test]
    fn otlp_sampler_ratio_must_be_in_unit_interval() {
        let mut cli = base_cli();
        cli.otlp.sampler_ratio = 0.0;
        assert!(
            cli.validate()
                .is_err_and(|err| err.contains("OTLP_SAMPLER_RATIO"))
        );
        cli.otlp.sampler_ratio = 1.5;
        assert!(cli.validate().is_err());
        cli.otlp.sampler_ratio = 0.5;
        assert!(cli.validate().is_ok());
        cli.otlp.sampler_ratio = 1.0;
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn rate_limit_must_be_positive() {
        let mut cli = base_cli();
        cli.rate_limit.auth_per_second = 0;
        assert!(
            cli.validate()
                .is_err_and(|err| err.contains("RATE_LIMIT_AUTH_PER_SECOND"))
        );
        cli.rate_limit.auth_per_second = 5;
        cli.rate_limit.auth_burst = 0;
        assert!(
            cli.validate()
                .is_err_and(|err| err.contains("RATE_LIMIT_AUTH_BURST"))
        );
    }

    #[test]
    fn login_phone_rate_limit_must_be_positive() {
        let mut cli = base_cli();
        cli.rate_limit.login_phone_window = "0s"
            .parse::<readable_duration::ReadableDuration>()
            .expect("parse");
        assert!(
            cli.validate()
                .is_err_and(|err| err.contains("RATE_LIMIT_LOGIN_PHONE_WINDOW"))
        );
        cli.rate_limit.login_phone_window = "5m"
            .parse::<readable_duration::ReadableDuration>()
            .expect("parse");
        cli.rate_limit.login_phone_max_attempts = 0;
        assert!(
            cli.validate()
                .is_err_and(|err| err.contains("RATE_LIMIT_LOGIN_PHONE_MAX_ATTEMPTS"))
        );
    }

    #[test]
    fn docs_disabled_by_default_and_overridable_via_env() {
        unsafe { remove_var("DOCS_ENABLED") };
        assert!(!base_cli().server.docs_enabled);
        unsafe { set_var("DOCS_ENABLED", "true") };
        let cli = Cli::try_parse_from(base_args()).expect("parse");
        unsafe { remove_var("DOCS_ENABLED") };
        assert!(cli.server.docs_enabled);
    }
}
