-- Initial schema (spec §5). Released migrations are never edited: add a new file instead.
--
-- Conventions:
--   * STRICT tables, so SQLite enforces column types.
--   * Timestamps are RFC 3339 UTC strings (e.g. 2026-11-19T21:00:00Z).
--   * Booleans are INTEGER 0/1.
--   * JSON columns are TEXT checked with json_valid().

-- Area › matter › topic paths.
CREATE TABLE topics (
    id     INTEGER PRIMARY KEY,
    area   TEXT NOT NULL CHECK (length(trim(area)) > 0),
    matter TEXT NOT NULL CHECK (length(trim(matter)) > 0),
    topic  TEXT NOT NULL CHECK (length(trim(topic)) > 0),
    UNIQUE (area, matter, topic)
) STRICT;

-- Current state of each question. Attempts keep their own snapshot in `responses`.
CREATE TABLE questions (
    id          TEXT PRIMARY KEY,                        -- content fingerprint (bank, #4)
    type        TEXT NOT NULL CHECK (type IN ('multiple', 'boolean')),
    stem        TEXT NOT NULL,
    options     TEXT NOT NULL CHECK (json_valid(options)),   -- [{ "id", "text" }, ...]
    answer_id   TEXT NOT NULL,
    explanation TEXT NOT NULL DEFAULT '',
    refs        TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(refs)),
    image       TEXT,                                    -- file name in <app data>/images/
    annulled    INTEGER NOT NULL DEFAULT 0 CHECK (annulled IN (0, 1)),
    contest     TEXT,                                    -- optional tag
    board       TEXT,                                    -- optional tag
    provenance  TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(provenance)),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
) STRICT;

CREATE TABLE question_topics (
    question_id TEXT NOT NULL REFERENCES questions (id) ON DELETE CASCADE,
    topic_id    INTEGER NOT NULL REFERENCES topics (id),
    PRIMARY KEY (question_id, topic_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX question_topics_by_topic ON question_topics (topic_id);

-- Fixed lists of questions: migrated cadernos, approved packages, practice sessions.
CREATE TABLE collections (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('imported', 'generated', 'practice')),
    contest    TEXT,
    board      TEXT,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE collection_items (
    collection_id TEXT NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    position      INTEGER NOT NULL CHECK (position >= 0),
    question_id   TEXT NOT NULL REFERENCES questions (id),
    PRIMARY KEY (collection_id, position)
) STRICT, WITHOUT ROWID;

CREATE INDEX collection_items_by_question ON collection_items (question_id);

-- Every attempt is kept forever (AGENTS.md rule 4).
CREATE TABLE attempts (
    id            TEXT PRIMARY KEY,
    source_kind   TEXT NOT NULL CHECK (source_kind IN ('collection', 'topic', 'errors', 'reviews')),
    source_ref    TEXT,                                  -- collection id, topic id, ... (attempts, #8)
    feedback_mode TEXT NOT NULL CHECK (feedback_mode IN ('study', 'exam')),
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    revision      INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
) STRICT;

CREATE INDEX attempts_by_finished_at ON attempts (finished_at);

CREATE TABLE responses (
    attempt_id  TEXT NOT NULL REFERENCES attempts (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL CHECK (position >= 0),
    question_id TEXT NOT NULL REFERENCES questions (id),
    snapshot    TEXT NOT NULL CHECK (json_valid(snapshot)),  -- question as shown in this attempt
    choice      TEXT,
    eliminated  TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(eliminated)),
    flagged     INTEGER NOT NULL DEFAULT 0 CHECK (flagged IN (0, 1)),
    revealed_at TEXT,
    answered_at TEXT,
    PRIMARY KEY (attempt_id, position)
) STRICT, WITHOUT ROWID;

CREATE INDEX responses_by_question ON responses (question_id);

-- Spaced-repetition state per question (learning, #21). `state` holds the scheduler's data;
-- `due_at` is a column so "due reviews" can be queried and indexed.
CREATE TABLE review_state (
    question_id TEXT PRIMARY KEY REFERENCES questions (id) ON DELETE CASCADE,
    due_at      TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (json_valid(state))
) STRICT;

CREATE INDEX review_state_by_due_at ON review_state (due_at);

-- Generated packages waiting for review (generation, #16–#18).
CREATE TABLE drafts (
    id          TEXT PRIMARY KEY,
    created_at  TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('review', 'imported', 'discarded')),
    request     TEXT NOT NULL CHECK (json_valid(request)),
    package     TEXT NOT NULL CHECK (json_valid(package)),
    blind_check TEXT CHECK (blind_check IS NULL OR json_valid(blind_check))
) STRICT;

-- Audit log of answer-key changes (re-check, #19).
CREATE TABLE key_changes (
    id            INTEGER PRIMARY KEY,
    question_id   TEXT NOT NULL REFERENCES questions (id) ON DELETE CASCADE,
    old_answer_id TEXT NOT NULL,
    new_answer_id TEXT NOT NULL,
    reason        TEXT NOT NULL DEFAULT '',
    source        TEXT NOT NULL,
    changed_at    TEXT NOT NULL
) STRICT;

CREATE INDEX key_changes_by_question ON key_changes (question_id);

-- App settings as JSON values (roles, CLI paths, backup folder, ...).
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL CHECK (json_valid(value))
) STRICT, WITHOUT ROWID;
