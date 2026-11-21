use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;

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

    fn load(&self) -> AgentCatalog {
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
    let first = cache.get();
    let second = cache.get();
    assert_eq!(first, second);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(first.agents[0].models[0].id, "m1");
}

#[test]
fn refresh_loads_again() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::ZERO);
    cache.get();
    cache.refresh();
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_refreshes_share_one_load() {
    let (cache, loads) = cache_with(ready(Agent::Codex, "m1"), Duration::from_millis(150));
    let cache = Arc::new(cache);
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || cache.refresh())
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
    let catalog = cache.get();
    assert_eq!(catalog.agents, vec![unavailable]);
}

#[test]
fn claude_source_has_no_list_and_explains_it_in_pt_br() {
    let catalog = claude::ClaudeSource.load();
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
