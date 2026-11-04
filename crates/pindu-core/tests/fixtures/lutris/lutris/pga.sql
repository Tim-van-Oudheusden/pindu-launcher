-- Lutris's pga.db, as lutris/database/schema.py creates it. The tests build pga.db from
-- this file, so the fixture stays readable in review.
CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, sortname TEXT, slug TEXT,
    installer_slug TEXT, parent_slug TEXT, platform TEXT, runner TEXT, executable TEXT,
    directory TEXT, updated DATETIME, lastplayed INTEGER, installed INTEGER,
    installed_at INTEGER, year INTEGER, configpath TEXT, has_custom_banner INTEGER,
    has_custom_icon INTEGER, has_custom_coverart_big INTEGER, playtime REAL, service TEXT,
    service_id TEXT, discord_id TEXT);
CREATE TABLE service_games (id INTEGER PRIMARY KEY, service TEXT, appid TEXT, name TEXT,
    slug TEXT, icon TEXT, logo TEXT, url TEXT, details TEXT, lutris_slug TEXT);
CREATE TABLE sources (id INTEGER PRIMARY KEY, uri TEXT UNIQUE);
CREATE TABLE categories (id INTEGER PRIMARY KEY, name TEXT UNIQUE);
CREATE TABLE games_categories (game_id INTEGER, category_id INTEGER);
CREATE TABLE saved_searches (id INTEGER PRIMARY KEY, name TEXT UNIQUE, search TEXT);

-- A native game with art, playtime (in hours), a favourite and a category.
INSERT INTO games (id, name, sortname, slug, platform, runner, directory, lastplayed,
    installed, installed_at, configpath, playtime)
VALUES (1, 'Celeste', '', 'celeste', 'Linux', 'linux', '/games/celeste', 1700000000, 1,
    1690000000, 'celeste-1690000000', 12.5);
-- A hidden Windows game without art or playtime.
INSERT INTO games (id, name, slug, platform, runner, directory, lastplayed, installed,
    configpath, playtime)
VALUES (2, 'The Witcher 3: Wild Hunt', 'the-witcher-3-wild-hunt', 'Windows', 'wine',
    '/games/witcher3', 0, 1, 'the-witcher-3-wild-hunt-1', 0.0);
-- A game in the library that is not installed (yet).
INSERT INTO games (id, name, slug, platform, runner, installed)
VALUES (3, 'Hollow Knight', 'hollow-knight', NULL, 'linux', 0);
-- A broken row.
INSERT INTO games (id, name, slug, installed) VALUES (4, NULL, NULL, 1);

INSERT INTO service_games (service, appid, name, slug)
VALUES ('gog', '1207664663', 'Not In The Library', 'not-in-the-library');

INSERT INTO categories (id, name) VALUES (1, 'favorite'), (2, '.hidden'), (3, 'Platformers');
INSERT INTO games_categories (game_id, category_id) VALUES (1, 1), (1, 3), (2, 2);
