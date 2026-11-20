//! The four AI roles and the agent, model and effort each one uses (spec §2 decision 2, §4.1).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::agent::{Agent, Effort};

/// Model every role starts with (the owner's choice: GPT-6.2-Sol on Codex, extra-high effort).
pub const DEFAULT_MODEL: &str = "gpt-6.2-sol";

/// What an AI job is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Role {
    Generate,
    BlindCheck,
    Recheck,
    Explain,
}

impl Role {
    pub const ALL: [Role; 4] = [
        Role::Generate,
        Role::BlindCheck,
        Role::Recheck,
        Role::Explain,
    ];

    /// pt-BR name shown in Configurações.
    pub fn label(self) -> &'static str {
        match self {
            Self::Generate => "Gerar questões",
            Self::BlindCheck => "Conferência às cegas",
            Self::Recheck => "Revisão sob demanda",
            Self::Explain => "Explicação sob demanda",
        }
    }
}

/// Agent, model and effort for one role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoleConfig {
    pub agent: Agent,
    pub model: String,
    pub effort: Effort,
}

impl RoleConfig {
    pub(crate) fn default_config() -> Self {
        Self {
            agent: Agent::Codex,
            model: DEFAULT_MODEL.to_owned(),
            effort: Effort::Xhigh,
        }
    }
}

/// The configuration of every role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoleSettings {
    pub generate: RoleConfig,
    pub blind_check: RoleConfig,
    pub recheck: RoleConfig,
    pub explain: RoleConfig,
}

impl Default for RoleSettings {
    /// Every role starts on Codex with `gpt-6.2-sol` at extra-high effort.
    fn default() -> Self {
        Self {
            generate: RoleConfig::default_config(),
            blind_check: RoleConfig::default_config(),
            recheck: RoleConfig::default_config(),
            explain: RoleConfig::default_config(),
        }
    }
}

impl RoleSettings {
    pub fn get(&self, role: Role) -> &RoleConfig {
        match role {
            Role::Generate => &self.generate,
            Role::BlindCheck => &self.blind_check,
            Role::Recheck => &self.recheck,
            Role::Explain => &self.explain,
        }
    }

    pub fn all_roles(&self) -> impl Iterator<Item = (Role, &RoleConfig)> {
        Role::ALL.into_iter().map(|role| (role, self.get(role)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_defaults_to_codex_gpt_6_2_sol_at_extra_high() {
        let settings = RoleSettings::default();
        for (role, config) in settings.all_roles() {
            assert_eq!(config.agent, Agent::Codex, "{role:?}");
            assert_eq!(config.model, "gpt-6.2-sol", "{role:?}");
            assert_eq!(config.effort, Effort::Xhigh, "{role:?}");
        }
    }

    #[test]
    fn roles_serialize_as_snake_case_keys() {
        assert_eq!(
            serde_json::to_value(Role::BlindCheck).unwrap(),
            "blind_check"
        );
        let json = serde_json::to_value(RoleSettings::default()).unwrap();
        assert!(json.get("blindCheck").is_some());
    }
}
