-- Saved filters gain the never-played toggle and a genre (MD-3): `never_played` keeps
-- games with no last played time, `genre` games of that genre, matched ignoring case;
-- NULL for any genre.
ALTER TABLE saved_filters ADD COLUMN never_played INTEGER NOT NULL DEFAULT 0;
ALTER TABLE saved_filters ADD COLUMN genre TEXT;
