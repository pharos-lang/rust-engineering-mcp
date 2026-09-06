//! Validated coverage accounting. Percentages are deliberately unrepresentable
//! when their denominator is zero (ADR-062).

use crate::{CheckOptions, CheckSelection, InvalidCheckOptions};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub const COVERAGE_DEFAULT_TIMEOUT_SECONDS: u64 = 300;
pub const COVERAGE_MAX_TIMEOUT_SECONDS: u64 = 3_600;
pub const COVERAGE_MAX_FILE_ROWS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageReportFormat {
    Json,
    Lcov,
    Html,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoverageSelection {
    pub package: Option<String>,
    pub workspace: bool,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target: Option<String>,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CoverageSelection")]
pub struct CoverageOptions {
    package: Option<String>,
    workspace: bool,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
    timeout_seconds: u64,
}

impl TryFrom<CoverageSelection> for CoverageOptions {
    type Error = InvalidCheckOptions;
    fn try_from(mut value: CoverageSelection) -> Result<Self, Self::Error> {
        if value.timeout_seconds == 0 {
            value.timeout_seconds = COVERAGE_DEFAULT_TIMEOUT_SECONDS;
        }
        if value.timeout_seconds > COVERAGE_MAX_TIMEOUT_SECONDS
            || (value.package.is_some() && value.workspace)
        {
            return Err(InvalidCheckOptions);
        }
        let checked = CheckOptions::try_from(CheckSelection {
            package: value.package,
            workspace: value.workspace,
            features: value.features,
            all_features: value.all_features,
            no_default_features: value.no_default_features,
            target: value.target,
            ..Default::default()
        })?;
        Ok(Self {
            package: checked.package().map(str::to_owned),
            workspace: checked.workspace(),
            features: checked.features().to_vec(),
            all_features: checked.all_features(),
            no_default_features: checked.no_default_features(),
            target: checked.target().map(str::to_owned),
            timeout_seconds: value.timeout_seconds,
        })
    }
}

impl CoverageOptions {
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
    pub fn workspace(&self) -> bool {
        self.workspace
    }
    pub fn features(&self) -> &[String] {
        &self.features
    }
    pub fn all_features(&self) -> bool {
        self.all_features
    }
    pub fn no_default_features(&self) -> bool {
        self.no_default_features
    }
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCoverageMetric;
impl fmt::Display for InvalidCoverageMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid coverage metric")
    }
}
impl Error for InvalidCoverageMetric {}

/// A non-zero coverage denominator. A scope that has no executable items is
/// represented by `None`, never by a percentage of zero or one hundred.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CoverageMetric {
    pub count: u64,
    pub covered: u64,
    pub percent_millionths: u32,
}
impl CoverageMetric {
    pub fn new(count: u64, covered: u64) -> Result<Option<Self>, InvalidCoverageMetric> {
        if count == 0 {
            return if covered == 0 {
                Ok(None)
            } else {
                Err(InvalidCoverageMetric)
            };
        }
        if covered > count {
            return Err(InvalidCoverageMetric);
        }
        let percent_millionths = covered
            .checked_mul(100_000_000)
            .ok_or(InvalidCoverageMetric)?
            .checked_div(count)
            .ok_or(InvalidCoverageMetric)?;
        Ok(Some(Self {
            count,
            covered,
            percent_millionths: percent_millionths
                .try_into()
                .map_err(|_| InvalidCoverageMetric)?,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CoverageMetrics {
    pub lines: Option<CoverageMetric>,
    pub regions: Option<CoverageMetric>,
    pub functions: Option<CoverageMetric>,
}
impl CoverageMetrics {
    pub fn new(
        lines: (u64, u64),
        regions: (u64, u64),
        functions: (u64, u64),
    ) -> Result<Self, InvalidCoverageMetric> {
        Ok(Self {
            lines: CoverageMetric::new(lines.0, lines.1)?,
            regions: CoverageMetric::new(regions.0, regions.1)?,
            functions: CoverageMetric::new(functions.0, functions.1)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoverageFile {
    pub path: String,
    pub package: String,
    pub metrics: CoverageMetrics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoveragePackage {
    pub name: String,
    pub metrics: CoverageMetrics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoverageSummary {
    pub aggregate: CoverageMetrics,
    pub packages: Vec<CoveragePackage>,
    pub files: Vec<CoverageFile>,
    pub files_omitted: u64,
}
