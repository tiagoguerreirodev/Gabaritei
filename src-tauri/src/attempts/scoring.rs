use serde::Serialize;
use ts_rs::TS;

/// Result of one question under the current key and annulment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Outcome {
    Correct,
    Wrong,
    Blank,
    Annulled,
}

/// A response counts as answered once it has a saved choice (`responses r`).
pub(crate) const ANSWERED_RESPONSE_SQL: &str = "r.choice IS NOT NULL";

/// SQL for when a response counts as given, over `responses r` joined to `attempts a`: the time
/// the user answered, or the attempt's finish time for a blank. Desempenho and the error
/// notebook both rank a question's responses by it, so they pick the same latest one.
pub const RESPONSE_TIME_SQL: &str = "COALESCE(r.answered_at, a.finished_at)";

/// SQL predicates for a non-annulled question q and its response r.
pub(crate) const HIT_SQL: &str = "r.choice IS NOT NULL AND r.choice = q.answer_id";
pub(crate) const MISS_SQL: &str = "r.choice IS NOT NULL AND r.choice <> q.answer_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionStatus {
    Valid,
    Annulled,
}

impl From<bool> for QuestionStatus {
    fn from(annulled: bool) -> Self {
        if annulled {
            Self::Annulled
        } else {
            Self::Valid
        }
    }
}

/// The one scoring rule. Annulment wins over everything; otherwise blank, correct or wrong.
pub fn outcome(choice: Option<&str>, answer_id: &str, status: QuestionStatus) -> Outcome {
    match (status, choice) {
        (QuestionStatus::Annulled, _) => Outcome::Annulled,
        (QuestionStatus::Valid, None) => Outcome::Blank,
        (QuestionStatus::Valid, Some(choice)) if choice == answer_id => Outcome::Correct,
        (QuestionStatus::Valid, Some(_)) => Outcome::Wrong,
    }
}

/// Totals for a set of outcomes. Annulled questions leave every denominator.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Score {
    pub correct: u32,
    pub wrong: u32,
    pub blank: u32,
    pub annulled: u32,
    /// Questions that count: everything but annulled ones.
    pub evaluated: u32,
    /// *Aproveitamento*: correct ÷ evaluated. `None` when nothing counts.
    pub achievement: Option<f64>,
    /// *Acerto entre respondidas*: correct ÷ (correct + wrong). `None` when nothing was answered.
    pub accuracy: Option<f64>,
}

impl Score {
    pub fn of(outcomes: impl IntoIterator<Item = Outcome>) -> Self {
        let mut score = Self::default();
        for outcome in outcomes {
            score.record(outcome);
        }
        score
    }

    pub(crate) fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Correct => self.correct += 1,
            Outcome::Wrong => self.wrong += 1,
            Outcome::Blank => self.blank += 1,
            Outcome::Annulled => self.annulled += 1,
        }
        self.evaluated = self.answered() + self.blank;
        self.achievement = Self::ratio(self.correct, self.evaluated);
        self.accuracy = Self::ratio(self.correct, self.answered());
    }

    pub(crate) fn answered(&self) -> u32 {
        self.correct + self.wrong
    }

    pub(crate) fn ratio(part: u32, whole: u32) -> Option<f64> {
        (whole > 0).then(|| f64::from(part) / f64::from(whole))
    }
}
