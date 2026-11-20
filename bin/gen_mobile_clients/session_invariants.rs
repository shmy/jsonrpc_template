//! 会话模板不变量：四门 `.inc` 的字符串断言（不启动语言运行时）。

const CORES: &[(&str, &str)] = &[
    ("dart", include_str!("templates/dart/client_core.inc")),
    (
        "typescript",
        include_str!("templates/typescript/client_core.inc"),
    ),
    ("swift", include_str!("templates/swift/client_core.inc")),
    ("kotlin", include_str!("templates/kotlin/client_core.inc")),
];

const RUNTIMES: &[(&str, &str)] = &[
    ("dart", include_str!("templates/dart/runtime.inc")),
    (
        "typescript",
        include_str!("templates/typescript/runtime.inc"),
    ),
    ("swift", include_str!("templates/swift/runtime.inc")),
    ("kotlin", include_str!("templates/kotlin/runtime.inc")),
];

#[test]
fn refresh_invalid_is_only_minus_32004() {
    for (lang, src) in RUNTIMES {
        assert!(
            src.contains("isRefreshInvalid") && src.contains("-32004"),
            "{lang}: isRefreshInvalid must be backed by -32004"
        );
    }
}

#[test]
fn access_authn_failure_codes_aligned() {
    for (lang, src) in RUNTIMES {
        for code in ["-32020", "-32021", "-32022"] {
            assert!(
                src.contains(code),
                "{lang}: runtime missing access-authn code {code}"
            );
        }
        assert!(
            src.contains("isAccessAuthnFailure"),
            "{lang}: missing isAccessAuthnFailure"
        );
    }
}

#[test]
fn refresh_rpc_documents_no_network_retry() {
    for (lang, src) in CORES {
        assert!(
            src.contains("不做传输层自动重试"),
            "{lang}: refresh path must document no transport retry"
        );
        assert!(
            src.contains("identity.account.refresh"),
            "{lang}: must call identity.account.refresh"
        );
    }
}

#[test]
fn used_access_stale_short_circuit() {
    for (lang, src) in CORES {
        assert!(
            src.contains("usedAccess"),
            "{lang}: must track usedAccess for stale short-circuit"
        );
    }
}

#[test]
fn single_flight_sync_claim() {
    let dart = include_str!("templates/dart/client_core.inc");
    assert!(
        dart.contains("Completer<TokenResponse>")
            && dart.contains("_refreshInFlight = gate.future"),
        "dart: must sync-claim Completer before async I/O"
    );

    let ts = include_str!("templates/typescript/client_core.inc");
    assert!(
        ts.contains("this.refreshInFlight = gate") && ts.contains("rotateRefresh()"),
        "ts: must assign refreshInFlight before starting rotateRefresh"
    );

    let kotlin = include_str!("templates/kotlin/client_core.inc");
    assert!(
        kotlin.contains("CompletableFuture") && kotlin.contains("synchronized(refreshMonitor)"),
        "kotlin: must register CompletableFuture under lock"
    );

    let swift = include_str!("templates/swift/client_core.inc");
    assert!(
        swift.contains("RefreshCoordinator") && swift.contains("OSAllocatedUnfairLock"),
        "swift: actor single-flight + unfair lock for tokens"
    );
}

#[test]
fn clear_tokens_bumps_epoch() {
    for (lang, src) in CORES {
        assert!(
            src.contains("sessionEpoch") || src.contains("_sessionEpoch"),
            "{lang}: must track sessionEpoch"
        );
        assert!(
            src.contains("sessionExpired"),
            "{lang}: clearTokens must support sessionExpired callback path"
        );
    }
}

#[test]
fn kotlin_apply_and_refresh_session_locked() {
    let kotlin = include_str!("templates/kotlin/client_core.inc");
    let apply = kotlin
        .split("fun applyTokens")
        .nth(1)
        .expect("applyTokens")
        .split("fun clearTokens")
        .next()
        .expect("apply body");
    assert!(
        apply.contains("synchronized(refreshMonitor)"),
        "kotlin applyTokens must share refreshMonitor with clearTokens"
    );

    let idx = kotlin.find("fun refreshSession").expect("refreshSession");
    let body = &kotlin[idx..(idx + 450).min(kotlin.len())];
    assert!(
        body.contains("synchronized(refreshMonitor)"),
        "kotlin refreshSession must enter refreshMonitor"
    );
    let inflight_at = body.find("refreshInFlight").expect("in-flight check");
    let token_at = body
        .find("refreshToken = refresh")
        .expect("assign refreshToken");
    assert!(
        inflight_at < token_at,
        "kotlin: check in-flight before overwriting refreshToken"
    );
}

#[test]
fn kotlin_no_fake_token_response_on_skip() {
    let kotlin = include_str!("templates/kotlin/client_core.inc");
    // 短路路径应 return@synchronized null / 提前 return，勿拼 expires_in = 0 的假票
    assert!(
        !kotlin.contains("expires_in = 0"),
        "kotlin: must not synthesize fake TokenResponse(expires_in=0)"
    );
}
