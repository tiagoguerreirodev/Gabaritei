//! The models, efforts and Fast Mode support each agent offers (spec §3.7).
//!
//! Each agent has a [`CatalogSource`]. The answers are cached for the app session and reloaded on
//! demand. A source that can't answer reports [`CatalogStatus::Unavailable`] with a pt-BR reason,
//! so the UI falls back to free text instead of failing.

pub mod claude;
pub mod codex;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use ts_rs::TS;

use super::agent::{Agent, Effort};
use super::roles::RoleConfig;
use super::settings::CliPaths;
use crate::error::{AppError, AppResult};

/// One model an agent can run, with the efforts it supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelOption {
    /// The value passed to the CLI's model flag.
    pub id: String,
    /// The name shown in the UI.
    pub label: String,
    pub efforts: Vec<Effort>,
    /// Whether the model has a Fast Mode tier (Codex only; the Claude list is never ready).
    pub supports_fast: bool,
}

/// Whether an agent's list could be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum CatalogStatus {
    Ready,
    /// The list couldn't be read. `reason` is pt-BR and safe to show.
    Unavailable {
        reason: String,
    },
}

/// One agent's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentCatalog {
    pub agent: Agent,
    pub status: CatalogStatus,
    pub models: Vec<ModelOption>,
}

/// The lists of every agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Catalog {
    pub agents: Vec<AgentCatalog>,
}

/// Where one agent's list comes from.
pub trait CatalogSource: Send + Sync {
    fn agent(&self) -> Agent;
    /// Reads the list, using the executable the user chose in `cli_paths` if any. Never fails:
    /// an unreadable list is `Unavailable`.
    fn load(&self, cli_paths: &CliPaths) -> AgentCatalog;
}

/// Session cache of the lists. Managed Tauri state.
pub struct CatalogCache {
    sources: Vec<Box<dyn CatalogSource>>,
    /// The loaded lists. Held only to read or swap them, never while a CLI runs.
    state: Mutex<Option<Catalog>>,
    /// Held for the whole of a load, so only one load runs at a time.
    load_lock: Mutex<()>,
    /// Counts completed loads, so a refresh that waited behind another one can reuse its result.
    generation: AtomicU64,
}

impl CatalogCache {
    pub fn new(sources: Vec<Box<dyn CatalogSource>>) -> Self {
        Self {
            sources,
            state: Mutex::new(None),
            load_lock: Mutex::new(()),
            generation: AtomicU64::new(0),
        }
    }

    fn load_all(&self, cli_paths: &CliPaths) -> Catalog {
        Catalog {
            agents: self
                .sources
                .iter()
                .map(|source| source.load(cli_paths))
                .collect(),
        }
    }

    fn lists(&self) -> std::sync::MutexGuard<'_, Option<Catalog>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reads the lists and stores them. Call with `load_lock` held.
    fn load_and_store(&self, cli_paths: &CliPaths) -> Catalog {
        let catalog = self.load_all(cli_paths);
        *self.lists() = Some(catalog.clone());
        self.generation.fetch_add(1, Ordering::SeqCst);
        catalog
    }

    /// The lists if they were already loaded. Never starts a CLI and never waits for a load in
    /// progress, so it is safe to call from a command that must not block.
    pub fn cached(&self) -> Option<Catalog> {
        self.lists().clone()
    }

    /// Forgets the lists (for example after the CLI paths changed).
    pub fn clear(&self) {
        *self.lists() = None;
    }

    /// The cached lists, loading them the first time.
    pub fn get(&self, cli_paths: &CliPaths) -> Catalog {
        if let Some(catalog) = self.cached() {
            return catalog;
        }
        let _loading = self.load_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(catalog) = self.cached() {
            return catalog;
        }
        self.load_and_store(cli_paths)
    }

    /// Reloads the lists. Callers that arrive while a reload is running share its result.
    pub fn refresh(&self, cli_paths: &CliPaths) -> Catalog {
        let seen = self.generation.load(Ordering::SeqCst);
        let _loading = self.load_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.generation.load(Ordering::SeqCst) != seen
            && let Some(catalog) = self.cached()
        {
            return catalog;
        }
        self.load_and_store(cli_paths)
    }
}

/// The lists for the UI: the cached ones, or freshly read ones when `refresh` is set.
pub fn catalog(cache: &CatalogCache, cli_paths: &CliPaths, refresh: bool) -> Catalog {
    if refresh {
        cache.refresh(cli_paths)
    } else {
        cache.get(cli_paths)
    }
}

/// Refuses a run whose model, or effort for that model, the agent's list doesn't offer. A list
/// that couldn't be read allows anything: the CLI itself will complain if the name is wrong.
pub fn ensure_listed(catalog: &Catalog, config: &RoleConfig, label: &str) -> AppResult<()> {
    let Some(agent) = catalog.agents.iter().find(|a| a.agent == config.agent) else {
        return Ok(());
    };
    if agent.status != CatalogStatus::Ready {
        return Ok(());
    }
    let Some(model) = agent.models.iter().find(|m| m.id == config.model) else {
        return Err(AppError::Invalid(format!(
            "O modelo “{}” de “{label}” não está mais disponível. Escolha outro em Configurações.",
            config.model
        )));
    };
    if !model.efforts.is_empty() && !model.efforts.contains(&config.effort) {
        return Err(AppError::Invalid(format!(
            "O esforço “{}” não existe para o modelo “{}” de “{label}”. Escolha outro em Configurações.",
            config.effort.as_str(),
            config.model
        )));
    }
    if config.fast && !model.supports_fast {
        return Err(AppError::Invalid(format!(
            "O modelo “{}” de “{label}” não tem modo rápido. Desligue o modo rápido em Configurações.",
            config.model
        )));
    }
    Ok(())
}

/// [`ensure_listed`] for each `(role, label)`, using the lists only if they are already loaded.
pub fn ensure_cached_listed(cache: &CatalogCache, roles: &[(&RoleConfig, &str)]) -> AppResult<()> {
    let Some(catalog) = cache.cached() else {
        return Ok(());
    };
    roles
        .iter()
        .try_for_each(|(config, label)| ensure_listed(&catalog, config, label))
}

#[cfg(test)]
mod tests;
