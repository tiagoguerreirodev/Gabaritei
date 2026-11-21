//! Codex's catalog source: the CLI's own model list (`codex debug models`).
//!
//! The output is large (it carries each model's prompt text), so only the few fields the app
//! needs are read and the rest is ignored. Verified in
//! `docs/superpowers/specs/2026-11-21-catalog-spike-findings.md`.

use std::path::Path;

use serde::Deserialize;

use super::{AgentCatalog, CatalogSource, CatalogStatus, ModelOption};
use crate::ai::agent::{Agent, Effort};
use crate::ai::discovery;
use crate::ai::settings::CliPaths;

/// The largest answer read from the CLI. The real list is under 1 MiB.
const MAX_OUTPUT: u64 = 8 << 20;

#[derive(Deserialize)]
struct RawCatalog {
    models: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    slug: String,
    display_name: Option<String>,
    visibility: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<RawLevel>,
    #[serde(default)]
    service_tiers: Vec<RawTier>,
}

#[derive(Deserialize)]
struct RawTier {
    id: String,
}

#[derive(Deserialize)]
struct RawLevel {
    effort: String,
}

/// The models `codex debug models` lists for the user, with the efforts the app knows.
pub(super) fn parse_models(json: &str) -> Result<Vec<ModelOption>, String> {
    let raw: RawCatalog = serde_json::from_str(json).map_err(|error| error.to_string())?;
    Ok(raw
        .models
        .into_iter()
        .filter(|model| model.visibility.as_deref() == Some("list"))
        .map(|model| {
            let mut efforts: Vec<Effort> = Vec::new();
            for level in &model.supported_reasoning_levels {
                if let Ok(effort) = serde_json::from_value::<Effort>(serde_json::Value::String(
                    level.effort.clone(),
                )) && !efforts.contains(&effort)
                {
                    efforts.push(effort);
                }
            }
            let supports_fast = model.service_tiers.iter().any(|tier| tier.id == "priority");
            ModelOption {
                label: model.display_name.unwrap_or_else(|| model.slug.clone()),
                id: model.slug,
                efforts,
                supports_fast,
            }
        })
        .collect())
}

/// Reads the model list from the `codex` CLI.
pub struct CodexSource;

fn unavailable(reason: &str) -> AgentCatalog {
    AgentCatalog {
        agent: Agent::Codex,
        status: CatalogStatus::Unavailable {
            reason: reason.to_owned(),
        },
        models: Vec::new(),
    }
}

impl CatalogSource for CodexSource {
    fn agent(&self) -> Agent {
        Agent::Codex
    }

    fn load(&self, cli_paths: &CliPaths) -> AgentCatalog {
        let custom = cli_paths.get(Agent::Codex).map(Path::new);
        let Some(cli) = discovery::find_cli(Agent::Codex, custom) else {
            return unavailable("O Codex não foi encontrado. Confira Configurações › Agentes.");
        };
        let Some(output) = discovery::capture_limited(&cli, &["debug", "models"], MAX_OUTPUT)
            .filter(|output| output.success)
        else {
            return unavailable(
                "Não foi possível ler a lista de modelos do Codex. Veja se ele está instalado e conectado.",
            );
        };
        match parse_models(&output.stdout) {
            Ok(models) => AgentCatalog {
                agent: Agent::Codex,
                status: CatalogStatus::Ready,
                models,
            },
            Err(_) => unavailable("A lista de modelos do Codex veio num formato inesperado."),
        }
    }
}
