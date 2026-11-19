-- The Wikidata item a refresh found for a game (#638): its QID, so each provider value
-- links to `https://www.wikidata.org/wiki/<QID>`, and when it was last fetched. A game
-- Wikidata has no item for has no row.
CREATE TABLE wikidata_items (
    game_id TEXT PRIMARY KEY REFERENCES games (id) ON DELETE CASCADE,
    qid TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
) WITHOUT ROWID;

-- The store and database IDs the item keeps, named by the Wikidata property that holds
-- them: PCGamingWiki `P6337`, IGDB `P5794`, SteamGridDB `P12561`, GOG `P2725`, Epic
-- `P6278`, Lutris `P7597`, Steam `P1733`. A property can hold several values.
CREATE TABLE wikidata_cross_ids (
    game_id TEXT NOT NULL REFERENCES wikidata_items (game_id) ON DELETE CASCADE,
    property TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (game_id, property, value)
) WITHOUT ROWID;
