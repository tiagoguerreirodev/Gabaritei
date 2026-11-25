use serde::Serialize;
use ts_rs::TS;

use crate::ordering::cmp_pt;

/// Tags that exist on collections, for the filter dropdowns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilterOptions {
    pub contests: Vec<String>,
    pub boards: Vec<String>,
}

impl FilterOptions {
    pub(crate) fn from_tags(
        tags: impl IntoIterator<Item = (Option<String>, Option<String>)>,
    ) -> Self {
        let (contests, boards): (Vec<_>, Vec<_>) = tags.into_iter().unzip();
        Self {
            contests: distinct(contests.into_iter().flatten()),
            boards: distinct(boards.into_iter().flatten()),
        }
    }
}

fn distinct(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut values: Vec<String> = values.collect();
    values.sort_by(|a, b| cmp_pt(a, b));
    values.dedup();
    values
}
