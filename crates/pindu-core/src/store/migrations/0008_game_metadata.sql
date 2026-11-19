-- Metadata by origin: each game's developer, genres and description from its source, an
-- online provider or the user, with when that origin last set it. A game shows the
-- user's value, then a provider's, then the source's; a sync writes source values only.
-- The `games` columns these replace held only what the last sync wrote, so their values
-- become source values and the columns go.
CREATE TABLE game_metadata (
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    -- `developer`, `genres` or `description`.
    field TEXT NOT NULL,
    -- `source`, `provider` or `user`.
    origin TEXT NOT NULL,
    -- The text; for `genres` a JSON array of strings. A field an origin has no value for
    -- has no row.
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (game_id, field, origin)
) WITHOUT ROWID;

INSERT INTO game_metadata (game_id, field, origin, value, updated_at)
SELECT id, 'developer', 'source', developer, CAST(strftime('%s', 'now') AS INTEGER)
FROM games WHERE developer IS NOT NULL;

INSERT INTO game_metadata (game_id, field, origin, value, updated_at)
SELECT id, 'genres', 'source', genres, CAST(strftime('%s', 'now') AS INTEGER)
FROM games WHERE genres <> '[]';

INSERT INTO game_metadata (game_id, field, origin, value, updated_at)
SELECT id, 'description', 'source', description, CAST(strftime('%s', 'now') AS INTEGER)
FROM games WHERE description IS NOT NULL;

ALTER TABLE games DROP COLUMN developer;
ALTER TABLE games DROP COLUMN genres;
ALTER TABLE games DROP COLUMN description;
