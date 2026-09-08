//! One source capture, one audit, independent policy and catalog observations.
use crate::security::{
    DenyObservation, ProjectDenyPort, SecurityCapture, SecurityError, compose_security,
};
use crate::{
    DependencyAuditPort, InspectionControl, InspectionError, ProjectInspectionPort,
    ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts, QualityProjectBackend,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::security::{DenyOptions, SecurityCompleteness, SecurityPolicy};
use rust_engineering_domain::supply_chain::*;
use rust_engineering_domain::{
    CargoVendorSnapshot, Clock, FreshnessState, ProjectRef, QualityArtifactDescriptor, SourceBundle,
};

pub trait SupplyFactsPort: Send + Sync {
    fn supply_facts(
        &self,
        source: &SourceBundle,
        vendor: Option<&CargoVendorSnapshot>,
        deny: Option<&DenyObservation>,
        control: &dyn InspectionControl,
    ) -> Result<SupplyGraph, SecurityError>;
}
pub trait SupplyCatalogPort: Send + Sync {
    /// One immutable authenticated catalog generation for this entire bounded slice.
    fn supply_catalog(
        &self,
        packages: &mut [SupplyPackage],
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<SupplyCatalog, SecurityError>;
}
pub trait SupplyPublisher: Send {
    fn publish_supply(
        &mut self,
        capture: &SecurityCapture,
        observation: &SupplyObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError>;
}
pub struct SupplyPorts<'a, E, A, C, P> {
    pub executor: &'a E,
    pub auditor: &'a A,
    pub catalog: &'a C,
    pub publisher: &'a mut P,
}
pub struct SupplyInputs<'a> {
    pub vendor: Option<&'a CargoVendorSnapshot>,
    pub policy: Option<&'a SecurityPolicy>,
    pub options: &'a DenyOptions,
}
pub struct PublishedSupply {
    pub observation: SupplyObservation,
    pub artifact: QualityArtifactDescriptor,
}
fn propagate_boundary(error: SecurityError) -> Result<(), SecurityError> {
    match error {
        SecurityError::Inspection(
            InspectionError::Project(crate::ProjectError::Cancelled)
            | InspectionError::Internal
            | InspectionError::Execution(
                crate::ExecutionError::Cancelled
                | crate::ExecutionError::CleanupUncertain
                | crate::ExecutionError::Infrastructure,
            ),
        )
        | SecurityError::Timeout => Err(error),
        _ => Ok(()),
    }
}
impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    pub fn supply_chain_durable<
        E: ProjectInspectionPort + ProjectDenyPort + SupplyFactsPort,
        A: DependencyAuditPort,
        K: SupplyCatalogPort,
        P: SupplyPublisher,
    >(
        &mut self,
        reference: &ProjectRef,
        inputs: SupplyInputs<'_>,
        ports: SupplyPorts<'_, E, A, K, P>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedSupply, SecurityError> {
        let capture = self.capture_security(reference, clock, control)?;
        let structure = ports.executor.inspect(&capture.source, control)?;
        control.check()?;
        let (audit_availability, audit) =
            match ports
                .auditor
                .audit(&capture.source, &structure, clock, control)
            {
                Ok(value) => (
                    if value.state == rust_engineering_domain::AuditState::Unavailable {
                        SupplyAvailability::Unavailable
                    } else if value.validation_complete {
                        SupplyAvailability::Available
                    } else {
                        SupplyAvailability::Partial
                    },
                    Some(value),
                ),
                Err(rust_engineering_domain::AuditDataError::Cancelled) => {
                    return Err(crate::ProjectError::Cancelled.into());
                }
                Err(rust_engineering_domain::AuditDataError::Timeout) => {
                    return Err(SecurityError::Timeout);
                }
                Err(rust_engineering_domain::AuditDataError::Internal) => {
                    return Err(InspectionError::Internal.into());
                }
                Err(_) => (SupplyAvailability::Invalid, None),
            };
        control.check()?;
        let (deny_availability, deny) =
            if let (Some(vendor), Some(policy)) = (inputs.vendor, inputs.policy) {
                match ports
                    .executor
                    .deny(&capture.source, vendor, policy, inputs.options, control)
                {
                    Ok(value) => {
                        value.validate()?;
                        (
                            if value.parse_complete {
                                SupplyAvailability::Available
                            } else {
                                SupplyAvailability::Partial
                            },
                            Some(value),
                        )
                    }
                    Err(error) => {
                        propagate_boundary(error)?;
                        (SupplyAvailability::Unavailable, None)
                    }
                }
            } else {
                (SupplyAvailability::NotConfigured, None)
            };
        control.check()?;
        if let Some(deny) = &deny
            && (deny.source_fingerprint != structure.source_fingerprint
                || !rust_engineering_domain::quality_runtime_matches(
                    &deny.runtime,
                    &structure.runtime,
                )
                || inputs
                    .vendor
                    .is_none_or(|v| v.tree_fingerprint != deny.vendor_fingerprint)
                || inputs
                    .policy
                    .is_none_or(|p| p.fingerprint() != &deny.policy_fingerprint))
        {
            return Err(SecurityError::InvalidMetadata);
        }

        let mut graph =
            ports
                .executor
                .supply_facts(&capture.source, inputs.vendor, deny.as_ref(), control)?;
        if graph.source_fingerprint != structure.source_fingerprint
            || graph.packages.len() > 4096
            || deny
                .as_ref()
                .is_some_and(|d| d.lock_fingerprint != graph.lock_fingerprint)
            || audit
                .as_ref()
                .and_then(|a| a.lock_fingerprint.as_ref())
                .is_some_and(|f| f != &graph.lock_fingerprint)
        {
            return Err(SecurityError::InvalidMetadata);
        }
        let total = graph.packages.len() as u32;
        graph.packages.truncate(128);
        let catalog = ports
            .catalog
            .supply_catalog(&mut graph.packages, clock, control)?;
        control.check()?;
        let policy_report = if let (Some(deny), Some(vendor), Some(policy)) =
            (deny, inputs.vendor, inputs.policy)
        {
            let combined = compose_security(
                &structure,
                audit
                    .clone()
                    .unwrap_or_else(rust_engineering_domain::AuditObservation::unavailable),
                deny,
                vendor,
                policy,
                clock,
            )?;
            Some((
                combined.completeness,
                SupplyDeny {
                    complete: combined.completeness == SecurityCompleteness::Complete,
                    policy_state: combined.policy_state,
                    findings: combined.findings,
                    findings_omitted: combined.findings_omitted,
                    policy_fingerprint: combined.deny.policy_fingerprint,
                    execution_fingerprint: combined.deny.execution_fingerprint,
                },
            ))
        } else {
            None
        };
        let complete = total <= 128
            && audit_availability == SupplyAvailability::Available
            && deny_availability == SupplyAvailability::Available
            && policy_report
                .as_ref()
                .is_some_and(|(c, _)| *c == SecurityCompleteness::Complete)
            && catalog.availability == SupplyAvailability::Available
            && catalog.evidence.as_ref().is_some_and(|e| {
                matches!(
                    e.freshness().state(),
                    FreshnessState::Fresh | FreshnessState::Aging
                )
            })
            && graph.packages.iter().all(|p| {
                p.active_features.is_some()
                    && p.declared_features.is_some()
                    && matches!(
                        p.yanked,
                        YankedFact::Yanked | YankedFact::NotYanked | YankedFact::NotApplicable
                    )
            });
        let observation = SupplyObservation {
            report: SupplyReport {
                source_fingerprint: graph.source_fingerprint,
                lock_fingerprint: graph.lock_fingerprint,
                packages_omitted: total - graph.packages.len() as u32,
                packages_total: total,
                packages: graph.packages,
                audit_availability,
                audit: audit.as_ref().map(SupplyAudit::from),
                deny_availability,
                deny: policy_report.map(|(_, d)| d),
                catalog,
                complete,
            },
            execution_fingerprint: structure.runtime.execution_fingerprint.clone(),
            runtime: structure.runtime,
        };
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifact = ports
            .publisher
            .publish_supply(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        if let Some(policy) = inputs.policy {
            policy
                .validate_at(clock.now().0)
                .map_err(|_| SecurityError::InvalidPolicy)?;
        }
        Ok(PublishedSupply {
            observation,
            artifact,
        })
    }
}
