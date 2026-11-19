-- How each launch of a game ended: a row when Pindu runs a game or hands it to its
-- launcher, finished when it ends. The newest few per game are kept; older rows are
-- deleted as new ones arrive.
CREATE TABLE launch_outcomes (
    id INTEGER PRIMARY KEY,
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    -- `hand-off`, `host`, `proton` or `runtime`.
    route TEXT NOT NULL,
    -- The Proton build a Windows game ran with, by the name a launch config stores.
    proton TEXT,
    started_at INTEGER NOT NULL,
    -- NULL while the launch is pending.
    ended_at INTEGER,
    -- `pending`, `not-started`, `exited`, `signalled`, `lost`, `handed-off`,
    -- `hand-off-failed` or `no-answer`.
    ending TEXT NOT NULL,
    -- The exit status for `exited`, the signal for `signalled`.
    code INTEGER,
    -- 1 or 0 where the session can tell whether a window of the game appeared, else NULL.
    window_seen INTEGER,
    -- The file name of the launch's log in the game's logs folder, if it kept one.
    log TEXT
);

CREATE INDEX launch_outcomes_by_game ON launch_outcomes (game_id, started_at);
