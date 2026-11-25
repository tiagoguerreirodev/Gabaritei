//! The models, efforts and Fast Mode support each agent offers (spec §3.7).
//!
//! Each agent has a [`CatalogSource`]. The answers are cached for the app session and reloaded on
//! demand. A source that can't answer reports [`CatalogStatus::Unavailable`] with a pt-BR reason,
//! so the UI falls back to free text instead of failing.

pub mod claude;
pub mod codex;

use crate::sync::lock;
use std::cmp::Reverse;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Catalog {
    pub agents: Vec<AgentCatalog>,
}

/// What the catalog offers for the role currently being edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoleOptions {
    pub efforts: Vec<Effort>,
    pub show_fast: bool,
    pub problem: Option<String>,
}

/// An edit that can change which model, effort and Fast Mode apply to a role.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum RoleChange {
    Agent { agent: Agent },
    Model { model: String },
}

impl Catalog {
    pub fn role_options(&self, config: &RoleConfig) -> RoleOptions {
        RoleOptions {
            efforts: self.effort_options(config),
            show_fast: self.show_fast(config),
            problem: self
                .role_problem(config)
                .map(|problem| problem.message(config, None)),
        }
    }

    pub fn change_role(&self, config: &RoleConfig, change: &RoleChange) -> RoleConfig {
        let mut config = config.clone();
        match change {
            RoleChange::Agent { agent } => {
                config.agent = *agent;
                config.model = self
                    .ready_list(*agent)
                    .and_then(|list| list.models.first())
                    .map_or_else(String::new, |model| model.id.clone());
                config.fast = false;
            }
            RoleChange::Model { model } => {
                config.model.clone_from(model);
                config.fast &= self.show_fast(&config);
            }
        }
        config.effort = closest_effort(&self.effort_options(&config), config.effort);
        config
    }

    fn ready_list(&self, agent: Agent) -> Option<&AgentCatalog> {
        self.agents
            .iter()
            .find(|list| list.agent == agent && list.status == CatalogStatus::Ready)
    }

    fn show_fast(&self, config: &RoleConfig) -> bool {
        if config.agent != Agent::Codex {
            return true;
        }
        self.ready_list(config.agent)
            .and_then(|list| list.find_model(&config.model))
            .is_none_or(|model| model.supports_fast)
    }

    fn effort_options(&self, config: &RoleConfig) -> Vec<Effort> {
        let Some(list) = self.ready_list(config.agent) else {
            return fallback_efforts(config.agent);
        };
        if let Some(model) = list.find_model(&config.model)
            && !model.efforts.is_empty()
        {
            return model.efforts.clone();
        }
        // A listed model without efforts takes the union of reported efforts in rank order.
        let reported: Vec<_> = Effort::ALL
            .into_iter()
            .filter(|effort| {
                list.models
                    .iter()
                    .any(|model| model.efforts.contains(effort))
            })
            .collect();
        if reported.is_empty() {
            fallback_efforts(config.agent)
        } else {
            reported
        }
    }

    fn role_problem(&self, config: &RoleConfig) -> Option<RoleProblem> {
        let list = self.ready_list(config.agent)?;
        let Some(model) = list.find_model(&config.model) else {
            return Some(RoleProblem::Model);
        };
        if !model.efforts.is_empty() && !model.efforts.contains(&config.effort) {
            return Some(RoleProblem::Effort);
        }
        if config.fast && !model.supports_fast {
            return Some(RoleProblem::Fast);
        }
        None
    }
}

impl AgentCatalog {
    fn find_model(&self, id: &str) -> Option<&ModelOption> {
        self.models.iter().find(|model| model.id == id)
    }
}

fn fallback_efforts(agent: Agent) -> Vec<Effort> {
    Effort::ALL
        .into_iter()
        .filter(|&effort| agent.supports_effort(effort))
        .collect()
}

fn closest_effort(supported: &[Effort], wanted: Effort) -> Effort {
    supported
        .iter()
        .copied()
        .min_by_key(|effort| {
            (
                effort.rank().abs_diff(wanted.rank()),
                Reverse(effort.rank()),
            )
        })
        .unwrap_or(wanted)
}

enum RoleProblem {
    Model,
    Effort,
    Fast,
}

impl RoleProblem {
    fn message(&self, config: &RoleConfig, context: Option<&str>) -> String {
        let model = &config.model;
        let (role, location) = context.map_or_else(
            || (String::new(), ""),
            |label| (format!(" de “{label}”"), " em Configurações"),
        );
        match self {
            Self::Model => format!(
                "O modelo “{model}”{role} não está mais disponível. Escolha outro{location}."
            ),
            Self::Effort => format!(
                "O esforço “{}” não existe para o modelo “{model}”{role}. Escolha outro{location}.",
                config.effort.as_str()
            ),
            Self::Fast => format!(
                "O modelo “{model}”{role} não tem modo rápido. Desligue o modo rápido{location}."
            ),
        }
    }
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
    state: Mutex<Option<(CliPaths, Catalog)>>,
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

    fn lists(&self) -> std::sync::MutexGuard<'_, Option<(CliPaths, Catalog)>> {
        lock(&self.state)
    }

    /// Reads the lists and stores them. Call with `load_lock` held.
    fn load_and_store(&self, cli_paths: &CliPaths) -> Catalog {
        let catalog = self.load_all(cli_paths);
        *self.lists() = Some((cli_paths.clone(), catalog.clone()));
        self.generation.fetch_add(1, Ordering::SeqCst);
        catalog
    }

    /// The lists if they were already loaded. Never starts a CLI and never waits for a load in
    /// progress, so it is safe to call from a command that must not block.
    fn cached(&self, cli_paths: &CliPaths) -> Option<Catalog> {
        self.with_cached(cli_paths, Clone::clone)
    }

    pub(super) fn with_cached<T>(
        &self,
        cli_paths: &CliPaths,
        read: impl FnOnce(&Catalog) -> T,
    ) -> Option<T> {
        self.lists()
            .as_ref()
            .filter(|(loaded_paths, _)| loaded_paths == cli_paths)
            .map(|(_, catalog)| read(catalog))
    }

    pub fn role_options(&self, cli_paths: &CliPaths, config: &RoleConfig) -> RoleOptions {
        self.with_cached(cli_paths, |catalog| catalog.role_options(config))
            .unwrap_or_else(|| Catalog::default().role_options(config))
    }

    pub fn change_role(
        &self,
        cli_paths: &CliPaths,
        config: &RoleConfig,
        change: &RoleChange,
    ) -> RoleConfig {
        self.with_cached(cli_paths, |catalog| catalog.change_role(config, change))
            .unwrap_or_else(|| Catalog::default().change_role(config, change))
    }

    /// Forgets the lists (for example after the CLI paths changed).
    pub fn clear(&self) {
        *self.lists() = None;
    }

    /// The cached lists, loading them the first time.
    pub fn get(&self, cli_paths: &CliPaths) -> Catalog {
        if let Some(catalog) = self.cached(cli_paths) {
            return catalog;
        }
        let _loading = lock(&self.load_lock);
        if let Some(catalog) = self.cached(cli_paths) {
            return catalog;
        }
        self.load_and_store(cli_paths)
    }

    /// Reloads the lists. Callers that arrive while a reload is running share its result.
    pub fn refresh(&self, cli_paths: &CliPaths) -> Catalog {
        let seen = self.generation.load(Ordering::SeqCst);
        let _loading = lock(&self.load_lock);
        if self.generation.load(Ordering::SeqCst) != seen
            && let Some(catalog) = self.cached(cli_paths)
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
    match catalog.role_problem(config) {
        Some(problem) => Err(AppError::Invalid(problem.message(config, Some(label)))),
        None => Ok(()),
    }
}

/// Validates the chosen roles without loading a CLI or waiting for an ongoing load.
pub(super) fn ensure_cached_listed(
    cache: &CatalogCache,
    cli_paths: &CliPaths,
    roles: &[(&RoleConfig, &str)],
) -> AppResult<()> {
    cache
        .with_cached(cli_paths, |catalog| {
            roles
                .iter()
                .try_for_each(|(config, label)| ensure_listed(catalog, config, label))
        })
        .unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests;
