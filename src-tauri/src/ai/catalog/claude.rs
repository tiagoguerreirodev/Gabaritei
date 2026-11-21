//! Claude Code's catalog source. The CLI has no command that lists its models or efforts
//! (verified in `docs/superpowers/specs/2026-11-21-catalog-spike-findings.md`), so Model and
//! Effort stay free text for Claude.

use super::{AgentCatalog, CatalogSource, CatalogStatus};
use crate::ai::agent::Agent;

pub struct ClaudeSource;

impl CatalogSource for ClaudeSource {
    fn agent(&self) -> Agent {
        Agent::Claude
    }

    fn load(&self) -> AgentCatalog {
        AgentCatalog {
            agent: Agent::Claude,
            status: CatalogStatus::Unavailable {
                reason: "O Claude Code não informa a lista de modelos; digite o nome.".to_owned(),
            },
            models: Vec::new(),
        }
    }
}
