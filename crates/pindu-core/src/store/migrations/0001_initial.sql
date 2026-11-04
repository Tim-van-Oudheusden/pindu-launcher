CREATE TABLE games (
    id TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL,
    native_id TEXT NOT NULL,
    title TEXT NOT NULL,
    sort_title TEXT NOT NULL,
    developer TEXT,
    platform TEXT CHECK (platform IN ('windows', 'linux')),
    installed INTEGER NOT NULL DEFAULT 0,
    install_path TEXT,
    size_bytes INTEGER,
    added_at INTEGER NOT NULL,
    last_played_at INTEGER,
    favourite INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0,
    pin_order INTEGER CHECK (pin_order BETWEEN 1 AND 3),
    user_removed INTEGER NOT NULL DEFAULT 0,
    missing_since INTEGER,
    launch_kind TEXT NOT NULL
        CHECK (launch_kind IN ('store-uri', 'host-command', 'desktop-entry', 'proton', 'runtime-native')),
    genres TEXT NOT NULL DEFAULT '[]',
    description TEXT,
    UNIQUE (source, native_id)
);

CREATE UNIQUE INDEX games_pin_order ON games (pin_order) WHERE pin_order IS NOT NULL;

CREATE TABLE launch_configs (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    executable TEXT NOT NULL,
    arguments TEXT NOT NULL DEFAULT '[]',
    working_dir TEXT,
    environment TEXT NOT NULL DEFAULT '{}',
    proton_version TEXT,
    prefix_path TEXT,
    mangohud INTEGER NOT NULL DEFAULT 0,
    gamemode INTEGER NOT NULL DEFAULT 0,
    wrappers TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE artwork (
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    layout TEXT NOT NULL CHECK (layout IN ('cover', 'hero', 'logo', 'icon')),
    origin TEXT NOT NULL CHECK (origin IN ('source', 'steamgriddb', 'user', 'generated')),
    path TEXT NOT NULL,
    PRIMARY KEY (game_id, layout, origin)
);

CREATE TABLE collections (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL
);

CREATE TABLE collection_members (
    collection_id INTEGER NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    PRIMARY KEY (collection_id, game_id)
);

CREATE INDEX collection_members_game ON collection_members (game_id);

CREATE TABLE play_sessions (
    id INTEGER PRIMARY KEY,
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL,
    ended_at INTEGER NOT NULL,
    duration_secs INTEGER NOT NULL
);

CREATE INDEX play_sessions_game ON play_sessions (game_id, started_at);

CREATE TABLE imported_playtime (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    total_secs INTEGER NOT NULL,
    last_played_at INTEGER
);

CREATE TABLE source_states (
    source TEXT PRIMARY KEY NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    resolved_location TEXT,
    user_override TEXT,
    last_sync INTEGER,
    errors TEXT NOT NULL DEFAULT '[]'
);
