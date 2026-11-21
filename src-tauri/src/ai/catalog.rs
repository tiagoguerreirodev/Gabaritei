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
    /// Reads the list. Never fails: an unreadable list is `Unavailable`.
    fn load(&self) -> AgentCatalog;
}

/// Session cache of the lists. Managed Tauri state.
pub struct CatalogCache {
    sources: Vec<Box<dyn CatalogSource>>,
    state: Mutex<Option<Catalog>>,
    /// Counts completed loads, so a refresh that waited behind another one can reuse its result.
    generation: AtomicU64,
}

impl CatalogCache {
    pub fn new(sources: Vec<Box<dyn CatalogSource>>) -> Self {
        Self {
            sources,
            state: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    fn load_all(&self) -> Catalog {
        Catalog {
            agents: self.sources.iter().map(|source| source.load()).collect(),
        }
    }

    /// The cached lists, loading them the first time.
    pub fn get(&self) -> Catalog {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(catalog) = state.as_ref() {
            return catalog.clone();
        }
        let catalog = self.load_all();
        self.generation.fetch_add(1, Ordering::SeqCst);
        *state = Some(catalog.clone());
        catalog
    }

    /// Reloads the lists. Callers that arrive while a reload is running share its result.
    pub fn refresh(&self) -> Catalog {
        let seen = self.generation.load(Ordering::SeqCst);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if self.generation.load(Ordering::SeqCst) != seen
            && let Some(catalog) = state.as_ref()
        {
            return catalog.clone();
        }
        let catalog = self.load_all();
        self.generation.fetch_add(1, Ordering::SeqCst);
        *state = Some(catalog.clone());
        catalog
    }
}

#[cfg(test)]
mod tests;
