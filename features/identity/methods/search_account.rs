use axum::extract::State;
use axum_jsonrpc::{Params, rpc};
use blob_kit::Blob;
use identity_contract::{Account, Id};
use rootcause::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toasty::Db;
use toasty::stmt::Expr;

use crate::shared::avatar_ops;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchAccountRequest {
    /// 姓名
    pub name: Option<String>,
    /// 手机号
    pub phone: Option<String>,
    /// 上一页最后一条的 id；首屏省略
    pub cursor: Option<Id>,
    /// 每页条数
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchAccountResponse {
    pub items: Vec<Account>,
    /// 本页最后一条 id；无更多结果时为 null
    pub next_cursor: Option<Id>,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// 搜索账号（PrivateRpc；任意登录可搜全库）
#[rpc("identity.account.search")]
#[tracing::instrument(skip(db, blob, request))]
pub async fn search_account(
    State(mut db): State<Db>,
    State(blob): State<Blob>,
    Params(request): Params<SearchAccountRequest>,
) -> Result<SearchAccountResponse> {
    let limit = request.limit.clamp(1, MAX_LIMIT);

    let mut predicates = Vec::new();
    if let Some(name) = request.name.filter(|s| !s.is_empty()) {
        predicates.push(Account::fields().name().starts_with(name));
    }
    if let Some(phone) = request.phone.filter(|s| !s.is_empty()) {
        predicates.push(Account::fields().phone().eq(phone));
    }
    if let Some(cursor) = request.cursor {
        // id desc：上一页末尾 id 之后 = 更小的 id
        predicates.push(Account::fields().id().lt(cursor));
    }

    let mut items = Account::filter(Expr::and_all(predicates))
        .order_by(Account::fields().id().desc())
        .limit(limit.saturating_add(1))
        .exec(&mut db)
        .await?;

    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|account| account.id)
    } else {
        None
    };

    for account in &mut items {
        avatar_ops::fill_avatar_for_wire(&blob, account);
    }

    Ok(SearchAccountResponse { items, next_cursor })
}
