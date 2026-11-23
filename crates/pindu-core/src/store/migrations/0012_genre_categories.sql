-- Genres become categories from a closed set, each provider's mapped onto it (#759). A
-- provider's genres were Wikidata's labels, in whatever language the refresh asked for,
-- with no item ID to map them by, so they go; the next Refresh Metadata stores mapped
-- ones. Heroic's store genres were kept as the store named them; the next sync writes
-- them mapped. The user's genres stay as they are.
DELETE FROM game_metadata WHERE field = 'genres' AND origin = 'provider';
DELETE FROM game_metadata WHERE field = 'genres' AND origin = 'source' AND game_id LIKE 'heroic-%';
