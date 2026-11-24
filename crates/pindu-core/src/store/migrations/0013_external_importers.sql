-- The state of each external importer (#831), the helpers of the `external` source that
-- `source_states`, keyed by source, cannot hold apart: whether the user turned it on
-- (off until then), the folder and the paths to watch its last probe found, and when its
-- last sync ran and what it could not read.
CREATE TABLE external_importers (
    helper_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    root TEXT,
    watch TEXT NOT NULL DEFAULT '[]',
    last_sync INTEGER,
    errors TEXT NOT NULL DEFAULT '[]'
) WITHOUT ROWID;
