//! 业务侧 OTel 计数器 / 直方图。无 meter provider（`OTLP_ENDPOINT` 为空）时为 noop。
//!
//! Meter 名是 instrumentation scope（固定），与 Resource 上的 `service.name`
//!（`OTLP_SERVICE_NAME`）是不同概念。

use opentelemetry::KeyValue;
use opentelemetry::global::meter;
use opentelemetry::metrics::{Counter, Histogram};
use std::sync::LazyLock;
use std::time::Duration;

const METER: &str = "jsonrpc";

static RPC_CALLS: LazyLock<Counter<u64>> = LazyLock::new(|| {
    meter(METER)
        .u64_counter("rpc.calls")
        .with_description("JSON-RPC method invocations")
        .build()
});

static RPC_BATCHES: LazyLock<Counter<u64>> = LazyLock::new(|| {
    meter(METER)
        .u64_counter("rpc.batches")
        .with_description("JSON-RPC batch requests")
        .build()
});

static RPC_BATCH_SIZE: LazyLock<Histogram<u64>> = LazyLock::new(|| {
    meter(METER)
        .u64_histogram("rpc.batch_size")
        .with_description("JSON-RPC batch request size (number of calls)")
        .build()
});

static RPC_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    meter(METER)
        .f64_histogram("rpc.duration")
        .with_description("JSON-RPC method duration in seconds")
        .with_unit("s")
        .build()
});

/// 记录一次 JSON-RPC 调用（成功或失败）及耗时。
pub fn record_rpc_call(method: &str, success: bool, error_code: Option<i32>, elapsed: Duration) {
    let mut attrs = vec![
        KeyValue::new("method", method.to_string()),
        KeyValue::new("success", success),
    ];
    if let Some(code) = error_code {
        attrs.push(KeyValue::new("error_code", i64::from(code)));
    }
    RPC_CALLS.add(1, &attrs);
    RPC_DURATION.record(elapsed.as_secs_f64(), &attrs);
}

/// 记录一次 batch：次数 +1，并记录该 batch 的调用条数分布。
pub fn record_rpc_batch(size: usize) {
    RPC_BATCHES.add(1, &[]);
    RPC_BATCH_SIZE.record(size as u64, &[]);
}
