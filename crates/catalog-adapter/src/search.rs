//! Scored FTS IDs and scalar version selection from the authoritative snapshot.
use super::{SqliteCatalogRepository, budget, records, sql};
use rusqlite::{OptionalExtension, params};
use rust_engineering_application::CatalogSearchRepository;
use rust_engineering_domain::{
    CatalogError, CatalogQuery, CrateSearchFilters, CrateSelection, KnownVersion, LexicalCandidate,
    MsrvVersion, SearchCrateFacts, SearchVersionFacts,
};

struct VersionRow {
    id: i64,
    version: String,
    yanked: bool,
    rust_version: Option<String>,
    license: Option<String>,
    published_at: Option<u64>,
}
impl VersionRow {
    fn known(&self) -> KnownVersion {
        KnownVersion {
            version: self.version.clone(),
            yanked: self.yanked,
            rust_version: self.rust_version.clone(),
            license: self.license.clone(),
        }
    }
}

impl CatalogSearchRepository for SqliteCatalogRepository {
    fn lexical_candidates(
        &self,
        query: &CatalogQuery,
    ) -> Result<Vec<LexicalCandidate>, CatalogError> {
        budget(&self.connection)?;
        let literal = query
            .text()
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let result = self.connection.prepare_cached(
            "SELECT c.name,bm25(crate_fts) AS score FROM crate_fts JOIN crates c ON c.id=crate_fts.rowid WHERE crate_fts MATCH ?1 ORDER BY score,c.name LIMIT ?2"
        ).map_err(sql)?.query_map(params![literal, query.limit()], |row| Ok(LexicalCandidate { name: row.get(0)?, bm25: row.get(1)? }))
            .map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        if result.iter().any(|candidate| !candidate.bm25.is_finite()) {
            return Err(CatalogError::Integrity);
        }
        Ok(result)
    }

    fn select(
        &self,
        name: &str,
        filters: &CrateSearchFilters,
    ) -> Result<CrateSelection, CatalogError> {
        if !records::valid_name(name) {
            return Err(CatalogError::InvalidInput);
        }
        budget(&self.connection)?;
        let scalar: Option<(i64, String, Option<String>)> = self
            .connection
            .query_row(
                "SELECT id,description,repository FROM crates WHERE name=?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((id, description, repository)) = scalar else {
            return Ok(CrateSelection::Missing);
        };
        // The sentinel 65th row detects a violated snapshot invariant without
        // loading feature/dependency graphs or unbounded version collections.
        let versions = self.connection.prepare_cached(
            "SELECT id,version,yanked,rust_version,license,published_at FROM versions WHERE crate_id=?1 LIMIT 65"
        ).map_err(sql)?.query_map([id], |row| {
            let published_at = row.get::<_, Option<i64>>(5)?.map(|time| u64::try_from(time).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5,time))).transpose()?;
            Ok(VersionRow { id: row.get(0)?, version: row.get(1)?, yanked: row.get(2)?, rust_version: row.get(3)?, license: row.get(4)?, published_at })
        }).map_err(sql)?.collect::<Result<Vec<_>,_>>().map_err(sql)?;
        if versions.len() > 64 {
            return Err(CatalogError::Budget);
        }
        if versions.is_empty() {
            return Err(CatalogError::InvalidSnapshot);
        }
        let version_count = versions.len() as u32;
        let mut versions = versions
            .into_iter()
            .map(|row| {
                semver::Version::parse(&row.version)
                    .map(|version| (version, row))
                    .map_err(|_| CatalogError::InvalidSnapshot)
            })
            .collect::<Result<Vec<_>, _>>()?;
        versions.sort_by(|a, b| b.0.cmp(&a.0));
        let latest_known_stable = versions
            .iter()
            .find(|(version, _)| version.pre.is_empty())
            .map(|(_, row)| row.known());
        let selected = versions.into_iter().find(|(version, row)| {
            (filters.allow_yanked || !row.yanked)
                && (filters.include_prerelease || version.pre.is_empty())
                && filters.msrv_lte.as_ref().is_none_or(|maximum| {
                    row.rust_version
                        .as_deref()
                        .and_then(|raw| MsrvVersion::parse(raw).ok())
                        .is_some_and(|actual| actual.components() <= maximum.components())
                })
        });
        let Some((_, selected)) = selected else {
            return Ok(CrateSelection::FilteredOut);
        };
        let known_advisory_ids = self.connection.prepare_cached(
            "SELECT advisory_id FROM advisories WHERE version_id=?1 ORDER BY advisory_id LIMIT 129"
        ).map_err(sql)?.query_map([selected.id], |row| row.get::<_,String>(0)).map_err(sql)?
            .collect::<Result<Vec<_>,_>>().map_err(sql)?;
        if known_advisory_ids.len() > 128 {
            return Err(CatalogError::Budget);
        }
        Ok(CrateSelection::Eligible(Box::new(SearchCrateFacts {
            name: name.to_owned(),
            description,
            repository,
            latest_known_stable,
            selected_version: SearchVersionFacts {
                version: selected.version,
                yanked: selected.yanked,
                rust_version: selected.rust_version,
                license: selected.license,
                published_at: selected.published_at,
                known_advisory_ids,
            },
            version_count,
        })))
    }
}
