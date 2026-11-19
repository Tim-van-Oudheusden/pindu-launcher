-- The itch app's butler.db, with the tables and columns butler's models
-- (github.com/itchio/butler, database/models; go-itchio's Game) give them through hades,
-- trimmed to what the importer reads plus the profile table it must never touch. The tests
-- build db/butler.db from this file, so the fixture stays readable in review.
CREATE TABLE profiles (id INTEGER PRIMARY KEY, api_key TEXT, last_connected DATETIME,
    user TEXT, developer BOOLEAN, press_user BOOLEAN);
CREATE TABLE install_locations (id TEXT PRIMARY KEY, path TEXT);
CREATE TABLE games (id INTEGER PRIMARY KEY, url TEXT, title TEXT, short_text TEXT,
    type TEXT, classification TEXT, cover_url TEXT, still_cover_url TEXT,
    created_at DATETIME, published_at DATETIME, min_price INTEGER, can_be_bought BOOLEAN,
    has_demo BOOLEAN, in_press_system BOOLEAN, windows TEXT, linux TEXT, osx TEXT,
    user_id INTEGER);
CREATE TABLE caves (id TEXT PRIMARY KEY, game_id INTEGER, external_game_id INTEGER,
    upload_id INTEGER, build_id INTEGER, morphing BOOLEAN, pinned BOOLEAN,
    installed_at DATETIME, last_touched_at DATETIME, seconds_run INTEGER,
    local_seconds_run INTEGER, local_last_run_at DATETIME, snoozed_at DATETIME,
    verdict TEXT, settings TEXT, installed_size INTEGER, install_location_id TEXT,
    install_folder_name TEXT, custom_install_folder TEXT);

INSERT INTO profiles (id, api_key, user) VALUES (7, 'not-a-real-key', '{}');
INSERT INTO install_locations (id, path) VALUES ('loc-default', '/home/me/.config/itch/apps');

-- A native game played on this machine and synced with itch.io: the server total is the
-- larger playtime, the local run the later time.
INSERT INTO games (id, url, title, type, classification, windows, linux, osx)
VALUES (1001, 'https://studio.itch.io/lighthouse', 'Lighthouse Keeper', 'default', 'game',
    'all', 'amd64', '');
INSERT INTO caves (id, game_id, installed_at, last_touched_at, seconds_run,
    local_seconds_run, local_last_run_at, installed_size, install_location_id,
    install_folder_name, custom_install_folder)
VALUES ('cave-lighthouse', 1001, '2025-01-10T09:00:00Z', '2025-03-01T20:15:00.123456789Z',
    5400, 1200, '2025-03-02T18:30:00Z', 734003200, 'loc-default', 'lighthouse-keeper', '');

-- A Windows-only game in a folder the user picked, never played.
INSERT INTO games (id, url, title, type, classification, windows, linux, osx)
VALUES (1002, 'https://maker.itch.io/frog-pond', 'Frog Pond', 'default', 'game',
    'all', '', '');
INSERT INTO caves (id, game_id, installed_at, seconds_run, local_seconds_run,
    installed_size, install_location_id, install_folder_name, custom_install_folder)
VALUES ('cave-frog', 1002, '2025-02-01T10:00:00Z', 0, 0, 1048576, '', '',
    '/mnt/games/frog-pond');

-- A game installed twice: the install run last on this machine is the one Play starts.
INSERT INTO games (id, url, title, type, classification, windows, linux, osx)
VALUES (1003, 'https://dev.itch.io/tiny-tower', 'Tiny Tower', 'default', 'game',
    '', 'all', '');
INSERT INTO caves (id, game_id, installed_at, last_touched_at, local_last_run_at,
    install_location_id, install_folder_name)
VALUES ('cave-tower-old', 1003, '2024-06-01T10:00:00Z', '2024-06-02T10:00:00Z',
    '2024-06-02T10:00:00Z', 'loc-default', 'tiny-tower');
INSERT INTO caves (id, game_id, installed_at, last_touched_at, local_last_run_at,
    install_location_id, install_folder_name)
VALUES ('cave-tower-new', 1003, '2024-07-01T10:00:00Z', '2024-08-02T10:00:00Z',
    '2024-08-02T10:00:00Z', 'loc-default', 'tiny-tower-1');

-- A soundtrack: something to listen to, not to play.
INSERT INTO games (id, url, title, type, classification, windows, linux, osx)
VALUES (1004, 'https://studio.itch.io/lighthouse-ost', 'Lighthouse Keeper OST', 'default',
    'soundtrack', '', '', '');
INSERT INTO caves (id, game_id, install_location_id, install_folder_name)
VALUES ('cave-ost', 1004, 'loc-default', 'lighthouse-keeper-ost');

-- An install whose game record is gone.
INSERT INTO caves (id, game_id, install_location_id, install_folder_name)
VALUES ('cave-orphan', 1005, 'loc-default', 'orphan');
