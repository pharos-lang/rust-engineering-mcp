use rust_engineering_domain::coverage::{CoverageMetric, CoverageOptions, CoverageSelection};

#[test]
fn zero_denominator_has_no_percent_bearing_metric() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(CoverageMetric::new(0, 0)?, None);
    assert!(CoverageMetric::new(0, 1).is_err());
    assert!(CoverageMetric::new(1, 2).is_err());
    let metric = CoverageMetric::new(3, 2)?.ok_or("expected nonzero metric")?;
    assert_eq!(metric.count, 3);
    assert_eq!(metric.covered, 2);
    Ok(())
}

#[test]
fn selection_is_closed_and_defaults_to_the_adr_budget() -> Result<(), Box<dyn std::error::Error>> {
    let options = CoverageOptions::try_from(CoverageSelection::default())?;
    assert_eq!(options.timeout_seconds(), 300);
    assert!(
        CoverageOptions::try_from(CoverageSelection {
            package: Some("member".into()),
            workspace: true,
            ..Default::default()
        })
        .is_err()
    );
    Ok(())
}
