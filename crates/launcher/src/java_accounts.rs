//! The install's Java Edition accounts as the menu shows them. The core owns them; Java joins
//! sign in with the selected one, independently of the Bedrock account.

/// A saved Java Edition account.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JavaAccount {
    pub id: String,
    pub name: String,
    /// The account's head picture, once the core has read it.
    pub picture_path: Option<String>,
}

/// The progress of adding a Java account.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum JavaAccountAdd {
    #[default]
    Idle,
    /// Waiting for the player to enter `code` at `uri`.
    AwaitingCode {
        uri: String,
        code: String,
    },
    /// The code was entered and the Java profile is being read.
    SigningIn,
    Failed(String),
}

impl JavaAccountAdd {
    /// Whether an add is under way, so its progress needs watching.
    pub fn in_progress(&self) -> bool {
        matches!(self, Self::AwaitingCode { .. } | Self::SigningIn)
    }
}

/// The Java accounts the core last reported.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JavaAccounts {
    pub accounts: Vec<JavaAccount>,
    pub selected: Option<String>,
    pub add: JavaAccountAdd,
    /// Set when the core could not be asked; the last good list stays shown.
    pub error: Option<String>,
    /// Set where the install has no Java accounts, as in a browser tab, so none are offered.
    pub unavailable: bool,
    /// Set while the core reads account pictures, so the open dialog asks again.
    pub pictures_pending: bool,
}

impl JavaAccounts {
    /// Takes the core's answer, keeping no earlier error.
    pub fn from_reply(reply: bridge::JavaAccounts) -> Self {
        use bridge::contract::{
            JAVA_ACCOUNT_ADD_AWAITING_CODE, JAVA_ACCOUNT_ADD_FAILED, JAVA_ACCOUNT_ADD_SIGNING_IN,
        };
        let add = match reply.add.state.as_str() {
            JAVA_ACCOUNT_ADD_AWAITING_CODE => JavaAccountAdd::AwaitingCode {
                uri: reply.add.verification_uri,
                code: reply.add.user_code,
            },
            JAVA_ACCOUNT_ADD_SIGNING_IN => JavaAccountAdd::SigningIn,
            JAVA_ACCOUNT_ADD_FAILED => JavaAccountAdd::Failed(reply.add.reason),
            _ => JavaAccountAdd::Idle,
        };
        Self {
            accounts: reply
                .accounts
                .into_iter()
                .map(|account| JavaAccount {
                    id: account.id,
                    name: account.name,
                    picture_path: (!account.picture.is_empty()).then_some(account.picture),
                })
                .collect(),
            selected: (!reply.selected.is_empty()).then_some(reply.selected),
            add,
            error: None,
            unavailable: false,
            pictures_pending: reply.pictures_pending,
        }
    }

    /// The selected account's position in the list.
    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_deref()?;
        self.accounts
            .iter()
            .position(|account| account.id == selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_becomes_the_menu_model() {
        let reply = bridge::JavaAccounts {
            accounts: vec![bridge::JavaAccount {
                id: "a".into(),
                name: "Alex".into(),
                picture: "/install/auth/java-accounts/a-head.png".into(),
            }],
            selected: "a".into(),
            add: bridge::JavaAccountAdd {
                state: bridge::contract::JAVA_ACCOUNT_ADD_AWAITING_CODE.into(),
                verification_uri: "https://www.microsoft.com/link".into(),
                user_code: "TESTCODE".into(),
                reason: String::new(),
            },
            pictures_pending: true,
        };
        let accounts = JavaAccounts::from_reply(reply);
        assert_eq!(accounts.selected_index(), Some(0));
        assert_eq!(
            accounts.accounts[0].picture_path.as_deref(),
            Some("/install/auth/java-accounts/a-head.png")
        );
        assert!(accounts.pictures_pending);
        assert!(accounts.add.in_progress());
        assert_eq!(
            accounts.add,
            JavaAccountAdd::AwaitingCode {
                uri: "https://www.microsoft.com/link".into(),
                code: "TESTCODE".into()
            }
        );
        let failed = JavaAccounts::from_reply(bridge::JavaAccounts {
            add: bridge::JavaAccountAdd {
                state: bridge::contract::JAVA_ACCOUNT_ADD_FAILED.into(),
                reason: "No Java".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(failed.add, JavaAccountAdd::Failed("No Java".into()));
        assert_eq!(failed.selected, None);
        assert_eq!(failed.accounts, []);
        assert!(!failed.pictures_pending);
    }
}
