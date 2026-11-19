-- Saved filters: a collection whose games are computed from a library view, its filter
-- toggles and search, rather than stored. The collection row keeps its name and place.
CREATE TABLE saved_filters (
    collection_id INTEGER PRIMARY KEY NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    -- `all`, `installed`, `favourites`, `play-history`, `source:<source>` or
    -- `collection:<id>`.
    view TEXT NOT NULL,
    installed INTEGER NOT NULL DEFAULT 0,
    not_installed INTEGER NOT NULL DEFAULT 0,
    favourites INTEGER NOT NULL DEFAULT 0,
    windows INTEGER NOT NULL DEFAULT 0,
    native INTEGER NOT NULL DEFAULT 0,
    -- The normalised search; empty for none.
    search TEXT NOT NULL DEFAULT ''
);
