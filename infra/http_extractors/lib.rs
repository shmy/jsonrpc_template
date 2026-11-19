//! Shared Axum extractors for the application template.

mod client_ip;

pub use client_ip::{ClientIp, HasTrustedProxies, client_ip_from_parts, resolve_client_ip};
