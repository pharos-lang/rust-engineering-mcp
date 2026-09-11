//! Owned, bounded source bytes. These values convey no host filesystem authority.
use std::collections::BTreeSet;

pub const SOURCE_MAX_ENTRIES: usize = 4096;
pub const SOURCE_MAX_DEPTH: usize = 32;
pub const SOURCE_MAX_PATH_BYTES: usize = 100;
pub const SOURCE_MAX_FILE_BYTES: usize = 1024 * 1024;
pub const SOURCE_MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceError {
    Invalid,
    Limits,
}

/// Validate the portable relative archive-name subset, without normalizing it.
pub fn validate_source_path(path: &str) -> Result<(), SourceError> {
    if path.is_empty()
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(SourceError::Invalid);
    }
    if path.len() > SOURCE_MAX_PATH_BYTES || path.split('/').count() > SOURCE_MAX_DEPTH {
        return Err(SourceError::Limits);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    path: String,
    bytes: Vec<u8>,
}
impl SourceFile {
    pub fn new(path: String, bytes: Vec<u8>) -> Result<Self, SourceError> {
        validate_source_path(&path)?;
        if bytes.len() > SOURCE_MAX_FILE_BYTES {
            return Err(SourceError::Limits);
        }
        Ok(Self { path, bytes })
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A captured set of exact file bytes, not an atomic filesystem snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBundle {
    files: Vec<SourceFile>,
    directories: Vec<String>,
}
impl SourceBundle {
    pub fn new(files: Vec<SourceFile>) -> Result<Self, SourceError> {
        Self::with_directories(files, Vec::new())
    }
    /// Preserves explicit (including empty) directories and adds implied parents.
    pub fn with_directories(
        mut files: Vec<SourceFile>,
        explicit: Vec<String>,
    ) -> Result<Self, SourceError> {
        if files.len() > SOURCE_MAX_ENTRIES || explicit.len() > SOURCE_MAX_ENTRIES {
            return Err(SourceError::Limits);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut entries = BTreeSet::new();
        let mut total = 0usize;
        for file in &files {
            if !entries.insert(file.path.as_str()) {
                return Err(SourceError::Invalid);
            }
            total = total
                .checked_add(file.bytes.len())
                .ok_or(SourceError::Limits)?;
            if total > SOURCE_MAX_TOTAL_BYTES {
                return Err(SourceError::Limits);
            }
        }
        let mut directories = BTreeSet::new();
        for directory in &explicit {
            validate_source_path(directory)?;
            if !directories.insert(directory.as_str()) {
                return Err(SourceError::Invalid);
            }
        }
        for path in files
            .iter()
            .map(|f| f.path.as_str())
            .chain(explicit.iter().map(String::as_str))
        {
            for (index, _) in path.match_indices('/') {
                directories.insert(&path[..index]);
            }
        }
        if directories
            .iter()
            .any(|directory| entries.contains(directory))
        {
            return Err(SourceError::Invalid);
        }
        if entries.len() + directories.len() > SOURCE_MAX_ENTRIES {
            return Err(SourceError::Limits);
        }
        let directories = directories.into_iter().map(str::to_owned).collect();
        Ok(Self { files, directories })
    }
    pub fn directories(&self) -> &[String] {
        &self.directories
    }
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_portable_relative_and_bounded() {
        for path in [
            "", "/a", "a/", "a//b", "a/../b", "./a", "a/./b", "a\\b", "é", "a\n", "a b", "a:b",
        ] {
            assert_eq!(
                validate_source_path(path),
                Err(SourceError::Invalid),
                "{path:?}"
            );
        }
        assert!(validate_source_path(&"a".repeat(100)).is_ok());
        assert_eq!(
            validate_source_path(&"a".repeat(101)),
            Err(SourceError::Limits)
        );
        assert!(validate_source_path(&vec!["a"; 32].join("/")).is_ok());
        assert_eq!(
            validate_source_path(&vec!["a"; 33].join("/")),
            Err(SourceError::Limits)
        );
    }
    fn file(path: &str, bytes: usize) -> SourceFile {
        SourceFile {
            path: path.to_owned(),
            bytes: vec![42; bytes],
        }
    }
    #[test]
    fn sorted_unique_and_no_file_directory_collisions() -> Result<(), SourceError> {
        let bundle = SourceBundle::new(vec![file("z", 0), file("a/b", 1)])?;
        assert_eq!(bundle.files()[0].path(), "a/b");
        assert_eq!(bundle.files()[0].bytes(), &[42]);
        for paths in [["a", "a"], ["a", "a/b"], ["a/b", "a"]] {
            assert_eq!(
                SourceBundle::new(paths.map(|p| file(p, 0)).to_vec()),
                Err(SourceError::Invalid)
            );
        }
        Ok(())
    }
    #[test]
    fn file_total_and_implied_directory_limits() {
        assert!(SourceFile::new("a".into(), vec![0; SOURCE_MAX_FILE_BYTES]).is_ok());
        assert_eq!(
            SourceFile::new("a".into(), vec![0; SOURCE_MAX_FILE_BYTES + 1]),
            Err(SourceError::Limits)
        );
        assert!(
            SourceBundle::new(
                (0..16)
                    .map(|n| file(&format!("f{n}"), SOURCE_MAX_FILE_BYTES))
                    .collect()
            )
            .is_ok()
        );
        assert_eq!(
            SourceBundle::new(
                (0..17)
                    .map(|n| file(&format!("f{n}"), SOURCE_MAX_FILE_BYTES))
                    .collect()
            ),
            Err(SourceError::Limits)
        );
        assert!(SourceBundle::new((0..4096).map(|n| file(&format!("f{n}"), 0)).collect()).is_ok());
        assert_eq!(
            SourceBundle::new((0..4096).map(|n| file(&format!("dir/f{n}"), 0)).collect()),
            Err(SourceError::Limits)
        );
    }
}

#[cfg(test)]
mod directory_tests {
    use super::*;
    #[test]
    fn explicit_empty_directories_and_implied_parents_are_unique_sorted() -> Result<(), SourceError>
    {
        let bundle = SourceBundle::with_directories(
            vec![SourceFile::new("a/file".into(), vec![])?],
            vec!["empty/leaf".into(), "a".into()],
        )?;
        assert_eq!(bundle.directories(), &["a", "empty", "empty/leaf"]);
        assert_eq!(bundle.files().len(), 1);
        Ok(())
    }
    #[test]
    fn directory_collisions_duplicates_names_and_shared_limit_are_enforced()
    -> Result<(), SourceError> {
        for directories in [
            vec!["a"],
            vec!["a/b"],
            vec!["bad/../path"],
            vec!["empty", "empty"],
        ] {
            assert_eq!(
                SourceBundle::with_directories(
                    vec![SourceFile::new("a".into(), vec![])?],
                    directories.into_iter().map(str::to_owned).collect()
                ),
                Err(SourceError::Invalid)
            );
        }
        let dirs: Vec<_> = (0..4096).map(|n| format!("d{n}")).collect();
        assert!(SourceBundle::with_directories(vec![], dirs.clone()).is_ok());
        assert_eq!(
            SourceBundle::with_directories(vec![SourceFile::new("file".into(), vec![])?], dirs),
            Err(SourceError::Limits)
        );
        assert_eq!(
            SourceBundle::with_directories(vec![], vec!["a".repeat(101)]),
            Err(SourceError::Limits)
        );
        Ok(())
    }
}
