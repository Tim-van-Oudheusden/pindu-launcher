-- The SteamGridDB game the user chose for a game in Change Artwork, for games that
-- SteamGridDB does not know by Steam app ID.
CREATE TABLE steamgriddb_matches (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    grid_id INTEGER NOT NULL
);
