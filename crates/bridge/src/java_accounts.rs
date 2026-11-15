//! The install's Java Edition accounts, which the core owns, through `java_accounts.v1`.

use serde::Deserialize;
#[cfg(not(target_arch = "wasm32"))]
use serde::Serialize;

/// A saved Java Edition account: its Java profile id and name.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct JavaAccount {
    pub id: String,
    pub name: String,
}

/// The progress of adding an account. `state` is one of the contract's `JAVA_ACCOUNT_ADD_*`
/// values; the code fields are set while it waits for the player, `reason` once it failed.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct JavaAccountAdd {
    pub state: String,
    #[serde(default)]
    pub verification_uri: String,
    #[serde(default)]
    pub user_code: String,
    #[serde(default)]
    pub reason: String,
}

/// Every action's answer: the accounts, the selected one and the sign-in under way.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct JavaAccounts {
    #[serde(default)]
    pub accounts: Vec<JavaAccount>,
    #[serde(default)]
    pub selected: String,
    #[serde(default)]
    pub add: JavaAccountAdd,
}

/// One `java_accounts.v1` action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavaAccountAction {
    List,
    Add,
    Cancel,
    Select(String),
    Remove(String),
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize)]
struct Request<'a> {
    action: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
}

#[cfg(not(target_arch = "wasm32"))]
impl JavaAccountAction {
    fn request(&self) -> Request<'_> {
        use crate::contract::{
            JAVA_ACCOUNTS_ADD, JAVA_ACCOUNTS_CANCEL, JAVA_ACCOUNTS_LIST, JAVA_ACCOUNTS_REMOVE,
            JAVA_ACCOUNTS_SELECT,
        };
        let (action, id) = match self {
            Self::List => (JAVA_ACCOUNTS_LIST, None),
            Self::Add => (JAVA_ACCOUNTS_ADD, None),
            Self::Cancel => (JAVA_ACCOUNTS_CANCEL, None),
            Self::Select(id) => (JAVA_ACCOUNTS_SELECT, Some(id.as_str())),
            Self::Remove(id) => (JAVA_ACCOUNTS_REMOVE, Some(id.as_str())),
        };
        Request { action, id }
    }
}

/// Sends one action and returns the accounts after it.
#[cfg(not(target_arch = "wasm32"))]
pub async fn java_accounts(
    socket_dir: &std::path::Path,
    action: &JavaAccountAction,
) -> Result<JavaAccounts, crate::BridgeError> {
    crate::account::call(
        socket_dir,
        crate::contract::METHOD_JAVA_ACCOUNTS,
        Some(action.request()),
    )
    .await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn actions_name_the_contract_actions() {
        let encoded = |action: JavaAccountAction| serde_json::to_value(action.request()).unwrap();
        assert_eq!(
            encoded(JavaAccountAction::List),
            serde_json::json!({ "action": crate::contract::JAVA_ACCOUNTS_LIST })
        );
        assert_eq!(
            encoded(JavaAccountAction::Select("abc".into())),
            serde_json::json!({ "action": crate::contract::JAVA_ACCOUNTS_SELECT, "id": "abc" })
        );
    }

    #[test]
    fn a_reply_decodes_with_and_without_a_sign_in_under_way() {
        let reply: JavaAccounts = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "accounts": [{ "id": "069a79f444e94726a5befca90e38aaf5", "name": "Notch" }],
            "selected": "069a79f444e94726a5befca90e38aaf5",
            "add": { "state": "awaiting_code", "verification_uri": "https://www.microsoft.com/link", "user_code": "TESTCODE" }
        }))
        .unwrap();
        assert_eq!(reply.accounts[0].name, "Notch");
        assert_eq!(reply.add.user_code, "TESTCODE");
        let idle: JavaAccounts = serde_json::from_value(serde_json::json!({
            "schema_version": 1, "accounts": [], "add": { "state": "idle" }
        }))
        .unwrap();
        assert!(idle.selected.is_empty() && idle.add.verification_uri.is_empty());
    }
}
