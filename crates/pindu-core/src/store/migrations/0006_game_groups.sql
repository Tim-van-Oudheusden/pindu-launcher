-- Game groups: copies of one game from several sources, shown as one game. A group is
-- Pindu's own note beside `games`; no `games` row or ID changes, so ungrouping gives back
-- exactly what was there before.
CREATE TABLE game_groups (
    id INTEGER PRIMARY KEY,
    -- The copy the user chose, one of the members; NULL lets the default order pick one.
    preferred_game_id TEXT REFERENCES games (id) ON DELETE SET NULL,
    -- Made from an exact store cross-reference rather than by the user.
    automatic INTEGER NOT NULL DEFAULT 0
);

-- A game is in at most one group.
CREATE TABLE game_group_members (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL REFERENCES game_groups (id) ON DELETE CASCADE
);

CREATE INDEX game_group_members_group ON game_group_members (group_id);

-- Pairs the user said are not the same game, so they are never grouped or suggested
-- again. The lower ID comes first.
CREATE TABLE game_splits (
    first_game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    second_game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    PRIMARY KEY (first_game_id, second_game_id),
    CHECK (first_game_id < second_game_id)
);

CREATE INDEX game_splits_second ON game_splits (second_game_id);

-- What a source says about a game beyond its `games` fields, refreshed on every sync:
-- Lutris's runner, the store record it links the game to, and the release year.
CREATE TABLE source_details (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    runner TEXT,
    service TEXT,
    service_id TEXT,
    year INTEGER,
    CHECK ((service IS NULL) = (service_id IS NULL))
);
