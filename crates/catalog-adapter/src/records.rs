use super::sql;
use rusqlite::{Connection, params};
use rust_engineering_domain::*;
use std::collections::HashSet;

pub(super) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.contains('\0')
}
fn optional(value: &Option<String>, max: usize) -> bool {
    value
        .as_ref()
        .is_none_or(|s| !s.trim().is_empty() && text(s, max))
}
fn timestamp(value: Option<u64>) -> bool {
    value.is_none_or(|v| v <= i64::MAX as u64)
}

pub(super) fn validate(crates: &[CrateRecord]) -> Result<(), CatalogError> {
    let mut names = HashSet::new();
    let mut entries = 0;
    if crates.len() > 1000 {
        return Err(CatalogError::Budget);
    }
    for krate in crates {
        if !valid_name(&krate.name)
            || !names.insert(&krate.name)
            || !text(&krate.description, 4096)
            || !optional(&krate.repository, 2048)
            || !timestamp(krate.updated_at)
            || krate.versions.is_empty()
            || krate.versions.len() > 64
        {
            return Err(CatalogError::InvalidInput);
        }
        let mut versions = HashSet::new();
        for version in &krate.versions {
            entries +=
                1 + version.features.len() + version.dependencies.len() + version.advisories.len();
            if entries > 100_000 {
                return Err(CatalogError::Budget);
            }
            if version.version.len() > 128
                || semver::Version::parse(&version.version).is_err()
                || !versions.insert(&version.version)
                || !optional(&version.license, 512)
                || !timestamp(version.published_at)
                || version.rust_version.as_ref().is_some_and(|v| {
                    v.len() > 32
                        || semver::Version::parse(&format!("{}.0", v)).is_err()
                            && semver::Version::parse(v).is_err()
                })
            {
                return Err(CatalogError::InvalidInput);
            }
            if version.features.len() > 128
                || version.dependencies.len() > 128
                || version.advisories.len() > 128
            {
                return Err(CatalogError::Budget);
            }
            let mut features = HashSet::new();
            for feature in &version.features {
                if !valid_feature(feature) || !features.insert(feature) {
                    return Err(CatalogError::InvalidInput);
                }
            }
            let mut dependencies = HashSet::new();
            for dependency in &version.dependencies {
                if !valid_name(&dependency.name)
                    || dependency.requirement.len() > 128
                    || semver::VersionReq::parse(&dependency.requirement).is_err()
                    || !dependencies.insert((&dependency.name, kind(dependency.kind)))
                {
                    return Err(CatalogError::InvalidInput);
                }
            }
            let mut advisories = HashSet::new();
            for advisory in &version.advisories {
                if !valid_name(advisory) || !advisories.insert(advisory) {
                    return Err(CatalogError::InvalidInput);
                }
            }
        }
    }
    Ok(())
}
fn kind(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Normal => "normal",
        DependencyKind::Build => "build",
        DependencyKind::Dev => "dev",
    }
}

pub(super) fn insert(connection: &Connection, crates: &[CrateRecord]) -> Result<(), CatalogError> {
    let mut sorted: Vec<_> = crates.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for krate in sorted {
        cached(
            connection,
            "INSERT INTO crates(name,description,repository,updated_at) VALUES(?1,?2,?3,?4)",
            params![
                krate.name,
                krate.description,
                krate.repository,
                sql_time(krate.updated_at)?
            ],
        )
        .map_err(sql)?;
        let crate_id = connection.last_insert_rowid();
        let mut versions = krate
            .versions
            .iter()
            .map(|version| {
                semver::Version::parse(&version.version)
                    .map(|key| (key, version))
                    .map_err(|_| CatalogError::InvalidInput)
            })
            .collect::<Result<Vec<_>, _>>()?;
        versions.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, version) in versions {
            cached(connection,"INSERT INTO versions(crate_id,version,yanked,rust_version,license,published_at) VALUES(?1,?2,?3,?4,?5,?6)",params![crate_id,version.version,version.yanked,version.rust_version,version.license,sql_time(version.published_at)?]).map_err(sql)?;
            let id = connection.last_insert_rowid();
            let mut features: Vec<_> = version.features.iter().collect();
            features.sort();
            for feature in features {
                cached(
                    connection,
                    "INSERT INTO features VALUES(?1,?2)",
                    params![id, feature],
                )
                .map_err(sql)?;
            }
            let mut dependencies: Vec<_> = version.dependencies.iter().collect();
            dependencies.sort_by_key(|d| (&d.name, kind(d.kind)));
            for dependency in dependencies {
                cached(
                    connection,
                    "INSERT INTO dependencies VALUES(?1,?2,?3,?4,?5)",
                    params![
                        id,
                        dependency.name,
                        dependency.requirement,
                        kind(dependency.kind),
                        dependency.optional
                    ],
                )
                .map_err(sql)?;
            }
            let mut advisories: Vec<_> = version.advisories.iter().collect();
            advisories.sort();
            for advisory in advisories {
                cached(
                    connection,
                    "INSERT INTO advisories VALUES(?1,?2)",
                    params![id, advisory],
                )
                .map_err(sql)?;
            }
        }
    }
    Ok(())
}

pub(super) fn all(connection: &Connection) -> Result<Vec<CrateRecord>, CatalogError> {
    let count: i64 = connection
        .query_row("SELECT count(*) FROM crates", [], |r| r.get(0))
        .map_err(sql)?;
    if count > 1000 {
        return Err(CatalogError::Budget);
    }
    // Validate row counts before allocating or joining hostile imported tables.
    let mut total = 0_i64;
    for query in [
        "SELECT count(*) FROM versions",
        "SELECT count(*) FROM features",
        "SELECT count(*) FROM dependencies",
        "SELECT count(*) FROM advisories",
    ] {
        let count: i64 = connection.query_row(query, [], |r| r.get(0)).map_err(sql)?;
        total = total.checked_add(count).ok_or(CatalogError::Budget)?;
        if total > 100_000 {
            return Err(CatalogError::Budget);
        }
    }
    let ids: Vec<i64> = connection
        .prepare("SELECT id FROM crates ORDER BY name")
        .map_err(sql)?
        .query_map([], |r| r.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    ids.into_iter().map(|id| get(connection, id)).collect()
}
pub(super) fn get(connection: &Connection, id: i64) -> Result<CrateRecord, CatalogError> {
    let mut krate = connection
        .query_row(
            "SELECT name,description,repository,updated_at FROM crates WHERE id=?1",
            [id],
            |r| {
                Ok(CrateRecord {
                    name: r.get(0)?,
                    description: r.get(1)?,
                    repository: r.get(2)?,
                    updated_at: read_time(r, 3)?,
                    versions: vec![],
                })
            },
        )
        .map_err(sql)?;
    let rows: Vec<(i64,VersionRecord)> = connection.prepare("SELECT id,version,yanked,rust_version,license,published_at FROM versions WHERE crate_id=?1 ORDER BY version LIMIT 65").map_err(sql)?.query_map([id],|r|Ok((r.get(0)?,VersionRecord { version:r.get(1)?,yanked:r.get(2)?,rust_version:r.get(3)?,license:r.get(4)?,published_at:read_time(r,5)?,features:vec![],dependencies:vec![],advisories:vec![] }))).map_err(sql)?.collect::<Result<_,_>>().map_err(sql)?;
    if rows.len() > 64 {
        return Err(CatalogError::Budget);
    }
    for (version_id, mut version) in rows {
        version.features = strings(
            connection,
            "SELECT name FROM features WHERE version_id=?1 ORDER BY name LIMIT 129",
            version_id,
        )?;
        version.advisories = strings(
            connection,
            "SELECT advisory_id FROM advisories WHERE version_id=?1 ORDER BY advisory_id LIMIT 129",
            version_id,
        )?;
        version.dependencies = connection.prepare("SELECT name,requirement,kind,optional FROM dependencies WHERE version_id=?1 ORDER BY name,kind LIMIT 129").map_err(sql)?.query_map([version_id],|r| {
            let raw:String = r.get(2)?;
            let kind = match raw.as_str() { "normal" => DependencyKind::Normal, "build" => DependencyKind::Build, "dev" => DependencyKind::Dev, _ => return Err(rusqlite::Error::InvalidQuery) };
            Ok(DependencyRecord { name:r.get(0)?,requirement:r.get(1)?,kind,optional:r.get(3)? })
        }).map_err(sql)?.collect::<Result<_,_>>().map_err(sql)?;
        krate.versions.push(version);
    }
    let mut ordered = krate
        .versions
        .into_iter()
        .map(|version| {
            semver::Version::parse(&version.version)
                .map(|key| (key, version))
                .map_err(|_| CatalogError::InvalidSnapshot)
        })
        .collect::<Result<Vec<_>, _>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    krate.versions = ordered.into_iter().map(|(_, version)| version).collect();
    Ok(krate)
}
fn strings(connection: &Connection, query: &str, id: i64) -> Result<Vec<String>, CatalogError> {
    connection
        .prepare(query)
        .map_err(sql)?
        .query_map([id], |r| r.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)
}

fn sql_time(value: Option<u64>) -> Result<Option<i64>, CatalogError> {
    value
        .map(i64::try_from)
        .transpose()
        .map_err(|_| CatalogError::InvalidInput)
}
fn read_time(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
        })
        .transpose()
}

fn valid_feature(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| unicode_ident::is_xid_start(c) || c == '_' || c.is_ascii_digit())
        && chars.all(|c| unicode_ident::is_xid_continue(c) || matches!(c, '-' | '+' | '.'))
}
fn cached<P: rusqlite::Params>(
    connection: &Connection,
    statement: &str,
    parameters: P,
) -> rusqlite::Result<usize> {
    connection.prepare_cached(statement)?.execute(parameters)
}
pub(super) fn summary(connection: &Connection, id: i64) -> Result<CrateSummary, CatalogError> {
    let (name, description) = connection
        .query_row(
            "SELECT name,description FROM crates WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    let versions = connection
        .prepare_cached(
            "SELECT version,yanked,rust_version,license FROM versions WHERE crate_id=?1 LIMIT 65",
        )
        .map_err(sql)?
        .query_map([id], |r| {
            Ok(KnownVersion {
                version: r.get(0)?,
                yanked: r.get(1)?,
                rust_version: r.get(2)?,
                license: r.get(3)?,
            })
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let version_count = versions.len() as u32;
    if version_count == 0 || version_count > 64 {
        return Err(CatalogError::InvalidSnapshot);
    }
    let latest_known = versions
        .into_iter()
        .map(|v| {
            semver::Version::parse(&v.version)
                .map(|key| (key, v))
                .map_err(|_| CatalogError::InvalidSnapshot)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max_by(|a, b| a.0.cmp(&b.0))
        .ok_or(CatalogError::InvalidSnapshot)?
        .1;
    Ok(CrateSummary {
        name,
        description,
        latest_known,
        version_count,
    })
}
