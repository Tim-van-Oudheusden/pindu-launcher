-- SQLite cannot change a CHECK constraint in place, so the artwork table is rebuilt to
-- allow the square layout. Nothing references artwork, so dropping it touches no other table.
CREATE TABLE artwork_new (
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    layout TEXT NOT NULL CHECK (layout IN ('cover', 'hero', 'logo', 'icon', 'square')),
    origin TEXT NOT NULL CHECK (origin IN ('source', 'steamgriddb', 'user', 'generated')),
    path TEXT NOT NULL,
    PRIMARY KEY (game_id, layout, origin)
);
INSERT INTO artwork_new (game_id, layout, origin, path)
    SELECT game_id, layout, origin, path FROM artwork;
DROP TABLE artwork;
ALTER TABLE artwork_new RENAME TO artwork;
