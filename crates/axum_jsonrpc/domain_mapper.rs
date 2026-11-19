//! Domain error mappers: feature crates register at `module()` time so Dynamic
//! `Report` can become a stable `-320xx` [`ErrorObject`].
//!
//! Lookup consults [`PENDING`] until [`seal_domain_mappers`] moves it into
//! [`SEALED`]. First dispatch must **not** seal — otherwise same-process unit
//! tests that dispatch before registering a fixture mapper panic.

use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::ErrorObject;

/// Map a dynamic rootcause report to a JSON-RPC error. Return `None` if this
/// mapper does not own the error (another mapper or `-32603` will handle it).
pub type DomainReportMapper = fn(&rootcause::Report) -> Option<ErrorObject>;

/// `Some` until [`seal_domain_mappers`]; `None` after ownership moves into [`SEALED`].
static PENDING: Mutex<Option<Vec<DomainReportMapper>>> = Mutex::new(Some(Vec::new()));
static SEALED: OnceLock<Vec<DomainReportMapper>> = OnceLock::new();

fn pending_lock() -> MutexGuard<'static, Option<Vec<DomainReportMapper>>> {
    PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Register a domain mapper. Call from feature `module()` before [`seal_domain_mappers`].
///
/// # Panics
/// If called after the mapper list has been sealed.
pub fn register_domain_mapper(mapper: DomainReportMapper) {
    let mut guard = pending_lock();
    #[allow(clippy::expect_used)]
    let pending = guard.as_mut().expect(
        "register_domain_mapper after mappers sealed \
         (must happen in Module::module(), before http_server seals)",
    );
    pending.push(mapper);
}

/// Seal the mapper list (idempotent). Call from the app shell before serving traffic.
pub fn seal_domain_mappers() {
    let _ = SEALED.get_or_init(|| pending_lock().take().unwrap_or_default());
}

fn invoke(mappers: &[DomainReportMapper], report: &rootcause::Report) -> Option<ErrorObject> {
    for mapper in mappers {
        if let Some(err) = mapper(report) {
            return Some(err);
        }
    }
    None
}

pub(crate) fn try_domain_mappers(report: &rootcause::Report) -> Option<ErrorObject> {
    if let Some(sealed) = SEALED.get() {
        return invoke(sealed, report);
    }

    let guard = pending_lock();
    if let Some(pending) = guard.as_ref() {
        return invoke(pending, report);
    }
    // Concurrent seal took PENDING; SEALED is (or soon will be) ready.
    drop(guard);
    invoke(SEALED.get().map(Vec::as_slice).unwrap_or(&[]), report)
}
