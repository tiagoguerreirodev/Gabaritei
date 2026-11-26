-- Attempts can be started from a combined filter (Banco). SQLite cannot alter a CHECK, so the
-- table is rebuilt. The runner turns foreign keys off around this migration; with them on,
-- DROP TABLE would delete every attempt row and cascade to `responses`.
CREATE TABLE attempts_new (
    id            TEXT PRIMARY KEY,
    source_kind   TEXT NOT NULL CHECK (source_kind IN ('collection', 'topic', 'errors', 'reviews', 'filter')),
    source_ref    TEXT,                                  -- collection id, topic id, or the filter as JSON
    feedback_mode TEXT NOT NULL CHECK (feedback_mode IN ('study', 'exam')),
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    revision      INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
) STRICT;

INSERT INTO attempts_new (id, source_kind, source_ref, feedback_mode, started_at, finished_at, revision)
SELECT id, source_kind, source_ref, feedback_mode, started_at, finished_at, revision FROM attempts;

DROP TABLE attempts;
ALTER TABLE attempts_new RENAME TO attempts;

CREATE INDEX attempts_by_finished_at ON attempts (finished_at);
