use serde::{Deserialize, Serialize};

/// Controls who can self-register once the instance already has an owner —
/// the very first account always gets created regardless of this, since
/// there'd otherwise be no admin able to set it in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistrationMode {
    /// No self-registration; accounts only via the CLI or an admin invite.
    Closed,
    /// Anyone who can reach the server can create an account.
    Open,
    /// Registering requires a valid, unused invite code from an admin.
    Invite,
}

impl RegistrationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RegistrationMode::Closed => "closed",
            RegistrationMode::Open => "open",
            RegistrationMode::Invite => "invite",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "closed" => Some(RegistrationMode::Closed),
            "open" => Some(RegistrationMode::Open),
            "invite" => Some(RegistrationMode::Invite),
            _ => None,
        }
    }
}
