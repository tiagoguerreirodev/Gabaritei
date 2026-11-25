use super::{GroupStats, Performance};
use crate::error::{AppError, AppResult};
use std::path::Path;

// ---- CSV export ------------------------------------------------------------------------------

/// UTF-8 byte order mark: Excel needs it to read accents correctly.
const BOM: &str = "\u{feff}";

/// One cell. Text a spreadsheet could run as a formula (`=`, `+`, `-`, `@`, tab, CR) gets a
/// leading apostrophe; separators, quotes and line breaks are quoted.
pub(super) fn cell(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if guarded.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// A ratio as a percentage with a decimal comma, or empty.
fn percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{:.1}", v * 100.0).replace('.', ","))
        .unwrap_or_default()
}

fn csv_row(level: &str, area: &str, matter: &str, topic: &str, stats: &GroupStats) -> String {
    let score = &stats.score;
    let fields = [
        cell(level),
        cell(area),
        cell(matter),
        cell(topic),
        score.correct.to_string(),
        score.wrong.to_string(),
        score.blank.to_string(),
        score.annulled.to_string(),
        score.evaluated.to_string(),
        percent(score.achievement),
        percent(score.accuracy),
        stats.available.to_string(),
        percent(stats.coverage),
    ];
    fields.join(";")
}

/// The area › matter › topic table as CSV (`;`-separated, UTF-8 with BOM, CRLF lines).
fn to_csv(performance: &Performance) -> String {
    let mut out = String::from(BOM);
    out.push_str(
        "Nível;Área;Matéria;Assunto;Certas;Erradas;Em branco;Anuladas;Avaliadas;\
         Aproveitamento (%);Acerto entre respondidas (%);Disponíveis;Cobertura (%)\r\n",
    );
    let mut line = |text: String| {
        out.push_str(&text);
        out.push_str("\r\n");
    };
    line(csv_row("Geral", "", "", "", &performance.overall));
    for area in &performance.areas {
        line(csv_row("Área", &area.name, "", "", &area.stats));
        for matter in &area.matters {
            line(csv_row(
                "Matéria",
                &area.name,
                &matter.name,
                "",
                &matter.stats,
            ));
            for topic in &matter.topics {
                line(csv_row(
                    "Assunto",
                    &area.name,
                    &matter.name,
                    &topic.name,
                    &topic.stats,
                ));
            }
        }
    }
    out
}

/// Writes the performance table to the chosen local file.
pub fn export_csv(path: &Path, performance: &Performance) -> AppResult<()> {
    std::fs::write(path, to_csv(performance))
        .map_err(|_| AppError::Invalid("Não foi possível salvar o arquivo.".into()))
}
