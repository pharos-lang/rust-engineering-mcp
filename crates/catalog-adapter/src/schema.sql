CREATE TABLE migrations(version INTEGER PRIMARY KEY, checksum TEXT NOT NULL) STRICT;
CREATE TABLE snapshots(id INTEGER PRIMARY KEY CHECK(id=1), sequence INTEGER NOT NULL CHECK(sequence>0), format_version INTEGER NOT NULL CHECK(format_version=1), provenance TEXT NOT NULL) STRICT;
CREATE TABLE crates(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, description TEXT NOT NULL, repository TEXT, updated_at INTEGER) STRICT;
CREATE TABLE versions(id INTEGER PRIMARY KEY, crate_id INTEGER NOT NULL REFERENCES crates(id), version TEXT NOT NULL, yanked INTEGER NOT NULL CHECK(yanked IN (0,1)), rust_version TEXT, license TEXT, published_at INTEGER, UNIQUE(crate_id,version)) STRICT;
CREATE TABLE features(version_id INTEGER NOT NULL REFERENCES versions(id), name TEXT NOT NULL, PRIMARY KEY(version_id,name)) STRICT;
CREATE TABLE dependencies(version_id INTEGER NOT NULL REFERENCES versions(id), name TEXT NOT NULL, requirement TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('normal','build','dev')), optional INTEGER NOT NULL CHECK(optional IN (0,1)), PRIMARY KEY(version_id,name,kind)) STRICT;
CREATE TABLE advisories(version_id INTEGER NOT NULL REFERENCES versions(id), advisory_id TEXT NOT NULL, PRIMARY KEY(version_id,advisory_id)) STRICT;
CREATE VIRTUAL TABLE crate_fts USING fts5(name, description, content='crates', content_rowid='id', tokenize='unicode61');
