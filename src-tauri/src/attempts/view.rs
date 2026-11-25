use super::*;

pub(super) fn collection_title(conn: &Connection, id: &CollectionId) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT title FROM collections WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}

pub(super) fn topic_title(conn: &Connection, id: TopicId) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT area || ' › ' || matter || ' › ' || topic FROM topics WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}

pub(super) fn title(
    conn: &Connection,
    kind: SourceKind,
    source_ref: Option<&SourceRef>,
) -> AppResult<String> {
    let found = match source_ref {
        Some(SourceRef::Collection(id)) => collection_title(conn, id)?,
        Some(SourceRef::Topic(id)) => topic_title(conn, *id)?,
        None => match kind {
            SourceKind::Collection | SourceKind::Topic => None,
            SourceKind::Errors => Some("Caderno de erros".into()),
            SourceKind::Reviews => Some("Revisões".into()),
        },
    };
    Ok(found.unwrap_or_else(|| "Sessão".into()))
}

/// The attempt as the frontend may see it.
pub fn load(conn: &Connection, id: &AttemptId) -> AppResult<AttemptView> {
    let header = load_header(conn, id)?;
    let mut stmt = conn.prepare(
        "SELECT r.position, r.question_id, r.snapshot, r.choice, r.eliminated, r.flagged,
                r.revealed_at IS NOT NULL, q.answer_id, q.explanation, q.refs, q.annulled,
                COALESCE(json_extract(q.provenance, '$.disputed'), 0)
         FROM responses r JOIN questions q ON q.id = r.question_id
         WHERE r.attempt_id = ?1 ORDER BY r.position",
    )?;
    let rows = stmt.query_map(params![id], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, QuestionId>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, bool>(10)?,
            r.get::<_, bool>(11)?,
        ))
    })?;
    let mut questions = Vec::new();
    let mut outcomes = Vec::new();
    for row in rows {
        let (
            position,
            question_id,
            snap,
            choice,
            eliminated,
            flagged,
            revealed,
            key,
            expl,
            refs,
            annulled,
            disputed,
        ) = row?;
        let snap: Snapshot = serde_json::from_str(&snap)?;
        let result = outcome(choice.as_deref(), &key, annulled);
        outcomes.push(result);
        // The only place the key, explanation and refs enter a view.
        let reveal = if revealed {
            Some(Reveal {
                answer_id: key,
                explanation: expl,
                refs: serde_json::from_str(&refs)?,
                annulled,
                outcome: result,
                disputed,
            })
        } else {
            None
        };
        questions.push(ResponseView {
            position,
            question_id,
            kind: snap.kind,
            stem: snap.stem,
            options: snap.options,
            image: snap.image,
            image_alt: snap.image_alt,
            choice,
            eliminated: serde_json::from_str(&eliminated)?,
            flagged,
            reveal,
        });
    }
    let score = header.finished_at.is_some().then(|| Score::of(outcomes));
    Ok(AttemptView {
        id: id.to_owned(),
        title: title(conn, header.source_kind, header.source_ref.as_ref())?,
        source_kind: header.source_kind,
        source_ref: header.source_ref,
        feedback_mode: header.mode,
        started_at: header.started_at,
        finished_at: header.finished_at,
        revision: header.revision,
        questions,
        score,
    })
}
