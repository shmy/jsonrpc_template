//! 账号 RPC 出站 DTO（白名单字段；不含 password / privileged / version）。

use account_id::Id;
use jiff::Timestamp;
use o2o::o2o;
use schemars::JsonSchema;
use serde::Serialize;

use crate::Account;

#[derive(Debug, Clone, Serialize, JsonSchema, o2o)]
#[from_owned(Account)]
pub struct AccountView {
    /// 账号ID
    pub id: Id,
    /// 姓名
    pub name: String,
    /// 手机号（已脱敏，例 `138****8888`）
    #[from(crate::phone::mask_for_wire(&@.phone))]
    pub phone: String,
    /// 头像：映射后仍是库内相对 path / 外链；绝对 URL 由 identity `fill_avatar` 填充
    pub avatar: Option<String>,
    /// 备注
    pub remark: Option<String>,
    /// 创建时间
    pub created_at: Timestamp,
    /// 更新时间
    pub updated_at: Timestamp,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn sample_account() -> Account {
        Account {
            id: Id::new_unchecked(1),
            name: "Alice".into(),
            phone: "13888888888".into(),
            password: "hash-must-not-leak".into(),
            privileged: true,
            avatar: Some("2026/01/abc.jpg".into()),
            remark: Some("r".into()),
            version: 9,
            created_at: Timestamp::from_second(1).unwrap(),
            updated_at: Timestamp::from_second(2).unwrap(),
        }
    }

    #[test]
    fn from_account_masks_phone_and_drops_secrets() {
        let view = AccountView::from(sample_account());
        assert_eq!(view.phone, "138****8888");
        assert_eq!(view.avatar.as_deref(), Some("2026/01/abc.jpg"));
        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "1",
                "name": "Alice",
                "phone": "138****8888",
                "avatar": "2026/01/abc.jpg",
                "remark": "r",
                "created_at": "1970-01-01T00:00:01Z",
                "updated_at": "1970-01-01T00:00:02Z",
            })
        );
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("password"));
        assert!(!obj.contains_key("privileged"));
        assert!(!obj.contains_key("version"));
    }
}
