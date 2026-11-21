use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::ai::roles::RoleConfig;
use crate::ai::settings::CliPaths;

/// A source that counts how often it is asked and answers with a fixed catalog.
struct FakeSource {
    agent: Agent,
    loads: Arc<AtomicUsize>,
    delay: Duration,
    result: AgentCatalog,
}

impl CatalogSource for FakeSource {
    fn agent(&self) -> Agent {
        self.agent
    }

    fn load(&self, _cli_paths: &CliPaths) -> AgentCatalog {
        self.loads.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        self.result.clone()
    }
}

fn ready(agent: Agent, id: &str) -> AgentCatalog {
    AgentCatalog {
        agent,
        status: CatalogStatus::Ready,
        models: vec![ModelOption {
            id: id.to_owned(),
            label: id.to_owned(),
            efforts: vec![Effort::Low, Effort::High],
            supports_fast: false,
        }],
    }
}

fn cache_with(result: AgentCatalog, delay: Duration) -> (CatalogCache, Arc<AtomicUsize>) {
    let loads = Arc::new(AtomicUsize::new(0));
    let source = FakeSource {
        agent: result.agent,
        loads: Arc::clone(&loads),
        delay,
        result,
    };
    (CatalogCache::new(vec![Box::new(source)]), loads)
}

#[test]
fn get_loads_each_source_once() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    let first = cache.get(&CliPaths::default());
    let second = cache.get(&CliPaths::default());
    assert_eq!(first, second);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(first.agents[0].models[0].id, "m1");
}

#[test]
fn refresh_loads_again() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    cache.get(&CliPaths::default());
    cache.refresh(&CliPaths::default());
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_refreshes_share_one_load() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::from_millis(150));
    let cache = Arc::new(cache);
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || cache.refresh(&CliPaths::default()))
        })
        .collect();
    let results: Vec<Catalog> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(results[0], results[1]);
}

#[test]
fn unavailable_source_is_kept_in_the_result_not_an_error() {
    let unavailable = AgentCatalog {
        agent: Agent::Claude,
        status: CatalogStatus::Unavailable {
            reason: "O Claude Code não informa a lista de modelos; digite o nome.".to_owned(),
        },
        models: Vec::new(),
    };
    let (cache, _) = cache_with(unavailable.clone(), Duration::ZERO);
    let catalog = cache.get(&CliPaths::default());
    assert_eq!(catalog.agents, vec![unavailable]);
}

#[test]
fn claude_source_has_no_list_and_explains_it_in_pt_br() {
    let catalog = claude::ClaudeSource.load(&CliPaths::default());
    assert_eq!(claude::ClaudeSource.agent(), Agent::Claude);
    assert_eq!(catalog.agent, Agent::Claude);
    assert!(catalog.models.is_empty());
    assert_eq!(
        catalog.status,
        CatalogStatus::Unavailable {
            reason: "O Claude Code não informa a lista de modelos; digite o nome.".to_owned()
        }
    );
}

// ---- Codex source -------------------------------------------------------------------------

const CODEX_JSON: &str = r#"{"models":[
  {"slug":"gpt-6.2-sol","display_name":"GPT-6.2-Sol","visibility":"list",
   "default_reasoning_level":"low",
   "supported_reasoning_levels":[{"effort":"low","description":"x"},{"effort":"high"},{"effort":"ultra"}],
   "service_tiers":[{"id":"priority","name":"Fast"}],
   "base_instructions":"TEXTO-LONGO","model_messages":{"instructions_template":"TEXTO-LONGO"}},
  {"slug":"gpt-reserve","display_name":"Reserve","visibility":"hide",
   "supported_reasoning_levels":[{"effort":"low"}]},
  {"slug":"gpt-6-luna","display_name":"GPT-6-Luna","visibility":"list",
   "supported_reasoning_levels":[{"effort":"turbo"},{"effort":"max"},{"effort":"max"}]}
]}"#;

#[test]
fn parse_keeps_only_listed_models() {
    let models = codex::parse_models(CODEX_JSON).unwrap();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["gpt-6.2-sol", "gpt-6-luna"]);
    assert_eq!(models[0].label, "GPT-6.2-Sol");
}

#[test]
fn parse_maps_efforts_and_drops_unknown_ones() {
    let models = codex::parse_models(CODEX_JSON).unwrap();
    assert_eq!(
        models[0].efforts,
        vec![Effort::Low, Effort::High, Effort::Ultra]
    );
    // "turbo" is not a known effort and the repeated "max" is kept once.
    assert_eq!(models[1].efforts, vec![Effort::Max]);
}

#[test]
fn parse_marks_models_that_have_a_priority_tier() {
    let models = codex::parse_models(CODEX_JSON).unwrap();
    assert!(models[0].supports_fast);
    assert!(!models[1].supports_fast);
}

#[test]
fn parse_rejects_text_that_is_not_the_catalog() {
    assert!(codex::parse_models("not json").is_err());
    assert!(codex::parse_models(r#"{"other":[]}"#).is_err());
}

#[cfg(unix)]
mod codex_cli {
    use super::*;
    use crate::ai::runner::tests::write_script;

    /// Loads the Codex list with `script` as the user's chosen executable.
    fn load_with(script: &std::path::Path) -> AgentCatalog {
        let paths = CliPaths {
            claude: None,
            codex: Some(script.to_string_lossy().into_owned()),
        };
        codex::CodexSource.load(&paths)
    }

    fn unavailable_reason(catalog: &AgentCatalog) -> &str {
        match &catalog.status {
            CatalogStatus::Unavailable { reason } => reason,
            CatalogStatus::Ready => panic!("expected unavailable"),
        }
    }

    #[test]
    fn source_runs_debug_models_and_lists_the_models() {
        let dir = tempfile::tempdir().unwrap();
        // Only answers to `debug models`, so a wrong argument list fails the test.
        let script = write_script(
            dir.path(),
            "codex",
            &format!(
                "[ \"$1\" = debug ] && [ \"$2\" = models ] || exit 2\ncat <<'JSON'\n{CODEX_JSON}\nJSON"
            ),
        );
        let catalog = load_with(&script);
        assert_eq!(catalog.agent, Agent::Codex);
        assert_eq!(catalog.status, CatalogStatus::Ready);
        assert_eq!(catalog.models.len(), 2);
    }

    #[test]
    fn missing_cli_is_unavailable_with_a_pt_br_reason() {
        let paths = CliPaths {
            claude: None,
            codex: Some("/nonexistent/codex".to_owned()),
        };
        let catalog = codex::CodexSource.load(&paths);
        assert!(catalog.models.is_empty());
        assert!(unavailable_reason(&catalog).contains("Codex não foi encontrado"));
    }

    #[test]
    fn failing_cli_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "codex", "echo 'erro' >&2\nexit 1");
        let catalog = load_with(&script);
        assert!(unavailable_reason(&catalog).contains("lista de modelos do Codex"));
    }

    #[test]
    fn invalid_output_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "codex", "echo 'not json'");
        let catalog = load_with(&script);
        assert!(unavailable_reason(&catalog).contains("formato inesperado"));
    }

    #[test]
    fn oversized_output_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "codex",
            "head -c 9000000 /dev/zero | tr '\\0' 'a'",
        );
        let catalog = load_with(&script);
        assert!(unavailable_reason(&catalog).contains("formato inesperado"));
    }
}

#[test]
fn catalog_reloads_only_when_asked() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    let paths = CliPaths::default();
    catalog(&cache, &paths, false);
    catalog(&cache, &paths, false);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    catalog(&cache, &paths, true);
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}

// ---- ensure_listed ------------------------------------------------------------------------

fn role(agent: Agent, model: &str, effort: Effort) -> RoleConfig {
    RoleConfig {
        agent,
        model: model.to_owned(),
        effort,
    }
}

fn catalog_of(agent_catalog: AgentCatalog) -> Catalog {
    Catalog {
        agents: vec![agent_catalog],
    }
}

#[test]
fn run_is_allowed_for_a_listed_model_and_effort() {
    let catalog = catalog_of(ready(Agent::Codex, "m1"));
    let config = role(Agent::Codex, "m1", Effort::High);
    assert!(ensure_listed(&catalog, &config, "Gerar questões").is_ok());
}

#[test]
fn run_is_blocked_for_a_model_the_catalog_no_longer_lists() {
    let catalog = catalog_of(ready(Agent::Codex, "m1"));
    let config = role(Agent::Codex, "gone", Effort::Low);
    let error = ensure_listed(&catalog, &config, "Gerar questões").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("gone"), "{message}");
    assert!(message.contains("não está mais disponível"), "{message}");
}

#[test]
fn run_is_blocked_for_an_effort_the_model_does_not_support() {
    // `ready` offers Low and High only.
    let catalog = catalog_of(ready(Agent::Codex, "m1"));
    let config = role(Agent::Codex, "m1", Effort::Max);
    let error = ensure_listed(&catalog, &config, "Gerar questões").unwrap_err();
    assert!(error.to_string().contains("esforço"), "{error}");
}

#[test]
fn run_is_allowed_for_any_model_when_the_catalog_is_unavailable() {
    let unavailable = AgentCatalog {
        agent: Agent::Claude,
        status: CatalogStatus::Unavailable {
            reason: "x".to_owned(),
        },
        models: Vec::new(),
    };
    let config = role(Agent::Claude, "qualquer-modelo", Effort::Max);
    assert!(ensure_listed(&catalog_of(unavailable), &config, "Gerar questões").is_ok());
}

#[test]
fn a_model_listed_without_efforts_accepts_any_effort() {
    let mut agent_catalog = ready(Agent::Codex, "m1");
    agent_catalog.models[0].efforts.clear();
    let config = role(Agent::Codex, "m1", Effort::Max);
    assert!(ensure_listed(&catalog_of(agent_catalog), &config, "Gerar questões").is_ok());
}

#[test]
fn run_is_allowed_when_the_agent_is_missing_from_the_catalog() {
    let catalog = catalog_of(ready(Agent::Codex, "m1"));
    let config = role(Agent::Claude, "sonnet", Effort::High);
    assert!(ensure_listed(&catalog, &config, "Gerar questões").is_ok());
}

#[test]
fn cached_is_empty_until_the_lists_are_loaded() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    assert!(cache.cached().is_none());
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    cache.get(&CliPaths::default());
    assert!(cache.cached().is_some());
}

#[test]
fn loaded_lists_block_a_stale_role_and_unloaded_ones_never_load_anything() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    let stale = role(Agent::Codex, "gone", Effort::Low);
    let listed = role(Agent::Codex, "m1", Effort::Low);

    // Nothing loaded yet: no check, and no CLI call from a command that must not block.
    assert!(ensure_cached_listed(&cache, &[(&stale, "Gerar questões")]).is_ok());
    assert_eq!(loads.load(Ordering::SeqCst), 0);

    cache.get(&CliPaths::default());
    assert!(ensure_cached_listed(&cache, &[(&listed, "Gerar questões")]).is_ok());
    let error = ensure_cached_listed(
        &cache,
        &[
            (&listed, "Gerar questões"),
            (&stale, "Conferência às cegas"),
        ],
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("Conferência às cegas"),
        "{error}"
    );
}

#[test]
fn cached_does_not_wait_for_a_load_in_progress() {
    let (cache, _) = cache_with(ready(Agent::Codex, "m1"), Duration::from_millis(400));
    let cache = Arc::new(cache);
    let loading = {
        let cache = Arc::clone(&cache);
        std::thread::spawn(move || cache.get(&CliPaths::default()))
    };
    std::thread::sleep(Duration::from_millis(80));
    let started = std::time::Instant::now();
    assert!(cache.cached().is_none());
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "cached() waited {:?} for the load",
        started.elapsed()
    );
    loading.join().unwrap();
    assert!(cache.cached().is_some());
}

#[test]
fn clear_forgets_the_lists_so_the_next_get_reads_them_again() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    cache.get(&CliPaths::default());
    cache.clear();
    assert!(cache.cached().is_none());
    cache.get(&CliPaths::default());
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}
