//! Bounded scalar and collection pages from the immutable SQLite generation.
use super::{SqliteCatalogRepository, budget, sql};
use rusqlite::{OptionalExtension, params};
use rust_engineering_application::CatalogInspectRepository;
use rust_engineering_domain::*;

fn pagination(
    request: &CrateInspectRequest,
    total: u32,
) -> Result<InspectPagination, CatalogError> {
    if request.offset > total {
        return Err(CatalogError::InvalidInput);
    }
    let returned = request.limit.min(total - request.offset);
    let end = request.offset + returned;
    Ok(InspectPagination {
        offset: request.offset,
        total,
        returned,
        next_offset: (end < total).then_some(end),
        omitted_by_output: 0,
    })
}
fn time(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, v)))
        .transpose()
}
impl CatalogInspectRepository for SqliteCatalogRepository {
    fn inspect_page(&self, request: &CrateInspectRequest) -> Result<InspectLookup, CatalogError> {
        request.validate()?;
        if let Some(version) = &request.version {
            semver::Version::parse(version).map_err(|_| CatalogError::InvalidInput)?;
        }
        budget(&self.connection)?;
        let scalar = self
            .connection
            .query_row(
                "SELECT id,description,repository,updated_at FROM crates WHERE name=?1",
                [&request.name],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        time(row, 3)?,
                    ))
                },
            )
            .optional()
            .map_err(sql)?;
        let Some((id, description, repository, updated_at)) = scalar else {
            return Ok(InspectLookup::CrateNotFound);
        };
        // Each correlated count stops at its sentinel. No collection graph is hydrated.
        let mut versions = self
            .connection
            .prepare_cached(
                "SELECT v.id,v.version,v.yanked,v.rust_version,v.license,v.published_at,
             (SELECT count(*) FROM (SELECT 1 FROM features WHERE version_id=v.id LIMIT 129)),
             (SELECT count(*) FROM (SELECT 1 FROM dependencies WHERE version_id=v.id LIMIT 129)),
             (SELECT count(*) FROM (SELECT 1 FROM advisories WHERE version_id=v.id LIMIT 129))
             FROM versions v WHERE v.crate_id=?1 LIMIT 65",
            )
            .map_err(sql)?
            .query_map([id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    InspectVersion {
                        version: row.get(1)?,
                        yanked: row.get(2)?,
                        rust_version: row.get(3)?,
                        license: row.get(4)?,
                        published_at: time(row, 5)?,
                        feature_count: row.get(6)?,
                        dependency_count: row.get(7)?,
                        advisory_count: row.get(8)?,
                    },
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        if versions.len() > 64
            || versions.iter().any(|(_, v)| {
                v.feature_count > 128 || v.dependency_count > 128 || v.advisory_count > 128
            })
        {
            return Err(CatalogError::Budget);
        }
        if versions.is_empty() {
            return Err(CatalogError::InvalidSnapshot);
        }
        let mut sorted = versions
            .drain(..)
            .map(|(id, v)| {
                semver::Version::parse(&v.version)
                    .map(|key| (key, id, v))
                    .map_err(|_| CatalogError::InvalidSnapshot)
            })
            .collect::<Result<Vec<_>, _>>()?;
        sorted.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.version.cmp(&a.2.version)));
        let latest_known_stable =
            sorted
                .iter()
                .find(|(key, _, _)| key.pre.is_empty())
                .map(|(_, _, v)| KnownVersion {
                    version: v.version.clone(),
                    yanked: v.yanked,
                    rust_version: v.rust_version.clone(),
                    license: v.license.clone(),
                });
        let overview = InspectOverview {
            name: request.name.clone(),
            description,
            repository,
            updated_at,
            latest_known_stable,
            version_count: sorted.len() as u32,
            documentation: InspectUnknown::default(),
            source: InspectUnknown::default(),
        };
        let selected = request
            .version
            .as_ref()
            .and_then(|version| sorted.iter().find(|(_, _, v)| &v.version == version));
        if request.version.is_some() && selected.is_none() {
            return Ok(InspectLookup::VersionNotFound);
        }
        let (data, page) = match request.section {
            InspectSection::Overview => (
                InspectPageData::Overview {
                    selected_version: selected.map(|(_, _, v)| v.clone()),
                },
                pagination(request, 1)?,
            ),
            InspectSection::Versions => {
                let page = pagination(request, overview.version_count)?;
                let items = sorted
                    .into_iter()
                    .skip(request.offset as usize)
                    .take(request.limit as usize)
                    .map(|(_, _, v)| v)
                    .collect();
                (InspectPageData::Versions { items }, page)
            }
            section => {
                let (_, version_id, version) = selected.ok_or(CatalogError::InvalidInput)?;
                let total = match section {
                    InspectSection::Features => version.feature_count,
                    InspectSection::Dependencies => version.dependency_count,
                    InspectSection::Advisories => version.advisory_count,
                    _ => return Err(CatalogError::InvalidInput),
                };
                let page = pagination(request, total)?;
                let data = match section {
                    InspectSection::Dependencies => {
                        let items = self.connection.prepare_cached("SELECT name,requirement,kind,optional FROM dependencies WHERE version_id=?1 ORDER BY name,kind LIMIT ?2 OFFSET ?3").map_err(sql)?
                        .query_map(params![version_id,request.limit,request.offset], |row| {
                            let kind:String=row.get(2)?;
                            let kind=match kind.as_str() { "normal"=>DependencyKind::Normal,"build"=>DependencyKind::Build,"dev"=>DependencyKind::Dev,_=>return Err(rusqlite::Error::InvalidQuery) };
                            Ok(DependencyRecord {name:row.get(0)?,requirement:row.get(1)?,kind,optional:row.get(3)?})
                        }).map_err(sql)?.collect::<Result<Vec<_>,_>>().map_err(sql)?;
                        InspectPageData::Dependencies {
                            version: version.clone(),
                            items,
                        }
                    }
                    InspectSection::Features | InspectSection::Advisories => {
                        let statement = if section == InspectSection::Features {
                            "SELECT name FROM features WHERE version_id=?1 ORDER BY name LIMIT ?2 OFFSET ?3"
                        } else {
                            "SELECT advisory_id FROM advisories WHERE version_id=?1 ORDER BY advisory_id LIMIT ?2 OFFSET ?3"
                        };
                        let items = self
                            .connection
                            .prepare_cached(statement)
                            .map_err(sql)?
                            .query_map(params![version_id, request.limit, request.offset], |row| {
                                row.get::<_, String>(0)
                            })
                            .map_err(sql)?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(sql)?;
                        if section == InspectSection::Features {
                            InspectPageData::Features {
                                version: version.clone(),
                                items,
                            }
                        } else {
                            InspectPageData::Advisories {
                                version: version.clone(),
                                items,
                            }
                        }
                    }
                    _ => return Err(CatalogError::InvalidInput),
                };
                (data, page)
            }
        };
        Ok(InspectLookup::Found {
            page: Box::new(InspectPage {
                overview,
                data,
                pagination: page,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::CatalogRepository;
    #[test]
    fn real_sqlite_count_and_version_sentinels_reject_invalid_staging()
    -> Result<(), Box<dyn std::error::Error>> {
        // Bypass open's validation only in this unit test to exercise the port's
        // defensive bounds against an invalid staging database, using real SQL.
        for table in ["versions", "features", "dependencies", "advisories"] {
            let provenance = Provenance::new(
                SourceKind::RegistrySnapshot,
                "sentinel".parse()?,
                None,
                None,
                IntegrityStatus::Verified,
                false,
            )?;
            let snapshot = SqliteCatalogRepository::build(
                1,
                provenance,
                &[CrateRecord {
                    name: "serde".into(),
                    description: "fixture".into(),
                    repository: None,
                    updated_at: None,
                    versions: vec![VersionRecord {
                        version: "1.0.0".into(),
                        yanked: false,
                        rust_version: None,
                        license: None,
                        published_at: None,
                        features: vec![],
                        dependencies: vec![],
                        advisories: vec![],
                    }],
                }],
            )?;
            let verified = SqliteCatalogRepository::open(&snapshot.bytes, &snapshot.manifest)?;
            let mut connection = rusqlite::Connection::open_in_memory()?;
            connection.deserialize_read_exact(
                rusqlite::MAIN_DB,
                snapshot.bytes.as_slice(),
                snapshot.bytes.len(),
                false,
            )?;
            for n in 0..129 {
                match table {
                    "versions" => {
                        connection.execute(
                            "INSERT INTO versions(crate_id,version,yanked) VALUES(1,?1,0)",
                            [format!("2.{n}.0")],
                        )?;
                    }
                    "features" => {
                        connection.execute(
                            "INSERT INTO features(version_id,name) VALUES(1,?1)",
                            [format!("f{n}")],
                        )?;
                    }
                    "dependencies" => {
                        connection.execute("INSERT INTO dependencies(version_id,name,requirement,kind,optional) VALUES(1,?1,'*','normal',0)",[format!("d{n}")])?;
                    }
                    "advisories" => {
                        connection.execute(
                            "INSERT INTO advisories(version_id,advisory_id) VALUES(1,?1)",
                            [format!("A{n}")],
                        )?;
                    }
                    _ => return Err("table".into()),
                }
            }
            let repo = SqliteCatalogRepository {
                connection,
                metadata: verified.metadata().clone(),
            };
            let query = CrateInspectRequest {
                name: "serde".into(),
                section: InspectSection::Overview,
                version: None,
                limit: 20,
                offset: 0,
                snapshot_fingerprint: None,
            };
            assert_eq!(
                repo.inspect_page(&query),
                Err(CatalogError::Budget),
                "{table}"
            );
        }
        Ok(())
    }
}
