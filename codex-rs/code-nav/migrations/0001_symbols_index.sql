CREATE TABLE IF NOT EXISTS indexed_files (
    path        TEXT    NOT NULL PRIMARY KEY,
    lang        TEXT    NOT NULL,
    mtime_secs  INTEGER NOT NULL,
    mtime_nanos INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS symbols (
    id        INTEGER PRIMARY KEY,
    file_path TEXT    NOT NULL REFERENCES indexed_files(path) ON DELETE CASCADE,
    name      TEXT    NOT NULL,
    kind      TEXT    NOT NULL,
    line      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
