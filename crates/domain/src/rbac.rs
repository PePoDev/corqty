use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Editor,
    Runner,
    Viewer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ViewOrganization,
    EditWorkflows,
    PublishWorkflows,
    RunWorkflows,
    ManageMembers,
    ManageServiceTokens,
    ManageSecrets,
    ManagePlugins,
    EnableShellExecutor,
}

impl Capability {
    pub const ALL: [Self; 9] = [
        Self::ViewOrganization,
        Self::EditWorkflows,
        Self::PublishWorkflows,
        Self::RunWorkflows,
        Self::ManageMembers,
        Self::ManageServiceTokens,
        Self::ManageSecrets,
        Self::ManagePlugins,
        Self::EnableShellExecutor,
    ];
}

impl Role {
    #[must_use]
    pub const fn allows(self, capability: Capability) -> bool {
        match self {
            Self::Admin => true,
            Self::Editor => matches!(
                capability,
                Capability::ViewOrganization
                    | Capability::EditWorkflows
                    | Capability::PublishWorkflows
                    | Capability::RunWorkflows
            ),
            Self::Runner => matches!(
                capability,
                Capability::ViewOrganization | Capability::RunWorkflows
            ),
            Self::Viewer => matches!(capability, Capability::ViewOrganization),
        }
    }

    #[must_use]
    pub fn capabilities(self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|capability| self.allows(*capability))
            .collect()
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Editor => "editor",
            Self::Runner => "runner",
            Self::Viewer => "viewer",
        }
    }
}

impl FromStr for Role {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "admin" => Ok(Self::Admin),
            "editor" => Ok(Self::Editor),
            "runner" => Ok(Self::Runner),
            "viewer" => Ok(Self::Viewer),
            _ => Err(DomainError::InvalidRole),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
