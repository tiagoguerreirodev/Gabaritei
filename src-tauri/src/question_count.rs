//! Counts accepted by the generation and topic-practice forms.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MIN_COUNT: u32 = 1;
const DEFAULT_COUNT: u32 = 10;
const SELECTION_DEFAULT_COUNT: u32 = 20;

#[derive(Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum CountContext {
    Generation,
    Topic { available: u32 },
    Selection { available: u32 },
}

#[derive(Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CountLimits {
    pub min: u32,
    pub max: u32,
    pub initial: u32,
}

impl CountContext {
    pub fn limits(self) -> CountLimits {
        let (max, default) = match self {
            Self::Generation => (crate::ai::generate::MAX_COUNT, DEFAULT_COUNT),
            Self::Topic { available } => (
                available.min(crate::attempts::MAX_TOPIC_SESSION),
                DEFAULT_COUNT,
            ),
            Self::Selection { available } => (
                available.min(crate::attempts::MAX_SELECTION_SESSION),
                SELECTION_DEFAULT_COUNT,
            ),
        };
        CountLimits {
            min: MIN_COUNT,
            max,
            initial: default.min(max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_limits_use_the_validated_range() {
        assert_eq!(
            CountContext::Generation.limits(),
            CountLimits {
                min: 1,
                max: 30,
                initial: 10
            }
        );
    }

    #[test]
    fn practice_respects_the_topic_size_and_the_session_limit() {
        for (available, max, initial) in [(0, 0, 0), (4, 4, 4), (250, 200, 10)] {
            assert_eq!(
                CountContext::Topic { available }.limits(),
                CountLimits {
                    min: 1,
                    max,
                    initial
                }
            );
        }
    }

    #[test]
    fn selection_defaults_to_twenty_within_what_is_available() {
        for (available, max, initial) in [(0, 0, 0), (4, 4, 4), (250, 200, 20)] {
            assert_eq!(
                CountContext::Selection { available }.limits(),
                CountLimits {
                    min: 1,
                    max,
                    initial
                }
            );
        }
    }
}
