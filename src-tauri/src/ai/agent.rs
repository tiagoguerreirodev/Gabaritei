//! The two CLI agents and the reasoning effort levels they share.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which local CLI runs a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Agent {
    /// Claude Code (`claude -p`).
    Claude,
    /// OpenAI Codex (`codex exec`).
    Codex,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::Claude, Agent::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The executable's base name (without a platform extension).
    pub fn binary(self) -> &'static str {
        self.as_str()
    }

    /// Display name in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

/// Reasoning effort. Both CLIs accept these names (verified in `docs/ai-cli.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Effort {
    Low,
    Medium,
    High,
    /// "Extra alto" in the UI.
    Xhigh,
    Max,
    /// Codex only; Claude's `--effort` stops at `max`.
    Ultra,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_what_the_clis_and_the_ui_use() {
        assert_eq!(serde_json::to_value(Agent::Codex).unwrap(), "codex");
        assert_eq!(serde_json::to_value(Effort::Xhigh).unwrap(), "xhigh");
        assert_eq!(serde_json::to_value(Effort::Ultra).unwrap(), "ultra");
        for effort in [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
            Effort::Ultra,
        ] {
            let json = serde_json::to_value(effort).unwrap();
            assert_eq!(json, effort.as_str());
        }
    }
}
