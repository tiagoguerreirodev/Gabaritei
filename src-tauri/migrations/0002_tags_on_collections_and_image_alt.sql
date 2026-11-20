-- Contest and board tags live on collections only (owner decision, 2026-11-19): a question can
-- belong to collections of several contests, so a single per-question tag can't describe it.
-- A question's contests are those of the collections that contain it.
ALTER TABLE questions DROP COLUMN contest;
ALTER TABLE questions DROP COLUMN board;

-- Alt text for the question image (every legacy image has one).
ALTER TABLE questions ADD COLUMN image_alt TEXT;
