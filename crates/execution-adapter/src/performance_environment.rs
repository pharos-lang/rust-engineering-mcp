//! Closed parsing and observation inputs for the M5 guest environment.
//!
//! The gateway, not project code, reads these kernel-provided bytes.  This
//! module never performs I/O: it validates the output of gateway phases and
//! constructs only the fixed CPUFreq paths those phases may read.
use std::collections::{BTreeMap, BTreeSet};

/// Shared ceiling for the small, kernel-owned hardware observations.
pub(super) const PROBE_CAPTURE_BYTES: usize = 64 * 1024;

/// The public hardware DTO permits at most 512 bytes of text. A sysfs attribute
/// contributes one final newline, so this is the largest complete CPU set the
/// gateway can prove under its existing 64 KiB probe-capture ceiling.
pub(super) const MAX_GUEST_CPU_IDS: usize = PROBE_CAPTURE_BYTES / (512 + 1);

/// The CPU model and logical CPUs the Linux guest made visible through
/// `/proc/cpuinfo`. This is deliberately guest-scoped: it says nothing about a
/// physical host or CPUs hidden by the container runtime.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GuestCpuTopology {
    pub(super) cpu_model: Option<String>,
    pub(super) logical_cpu_ids: Vec<u16>,
    pub(super) cpu_ids_complete: bool,
}

/// The only representation allowed to reach the dynamic CPUFreq argv. Its
/// field stays private so callers cannot substitute a peer-supplied path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GovernorPaths(Vec<String>);

impl GovernorPaths {
    pub(super) fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }
}

/// Parses `/proc/cpuinfo` as the guest kernel printed it. ARM64 commonly lacks
/// `model name`, so its implementer/part/variant/revision tuple is retained
/// verbatim when complete. Duplicate or malformed processor IDs never become
/// paths; an incomplete topology is not fit for a whole-guest governor claim.
pub(super) fn parse_cpuinfo(bytes: &[u8]) -> GuestCpuTopology {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return GuestCpuTopology::default();
    };
    let mut models = BTreeSet::new();
    let mut model_malformed = false;
    let mut parts: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut parts_malformed = false;
    let mut ids = BTreeSet::new();
    let mut saw_processor = false;
    let mut malformed_processor = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "processor" => {
                saw_processor = true;
                if let Ok(id) = value.parse::<u16>() {
                    if !ids.insert(id) {
                        malformed_processor = true;
                    }
                } else {
                    malformed_processor = true;
                }
            }
            "model name" | "Model" => {
                if valid_text(value) {
                    models.insert(value.to_owned());
                } else {
                    model_malformed = true;
                }
            }
            "CPU implementer" | "CPU part" | "CPU variant" | "CPU revision" => {
                if valid_text(value) {
                    parts.entry(key).or_default().insert(value.to_owned());
                } else {
                    parts_malformed = true;
                }
            }
            _ => (),
        }
    }
    let model = if !model_malformed && models.len() == 1 {
        models.iter().next().cloned()
    } else if models.is_empty()
        && !model_malformed
        && !parts_malformed
        && parts.len() == 4
        && parts.values().all(|values| values.len() == 1)
    {
        Some(
            parts
                .iter()
                .filter_map(|(key, values)| values.first().map(|value| format!("{key}={value}")))
                .collect::<Vec<_>>()
                .join(" "),
        )
    } else {
        None
    };
    GuestCpuTopology {
        cpu_model: model.filter(|value| valid_text(value)),
        logical_cpu_ids: ids.into_iter().collect(),
        cpu_ids_complete: saw_processor && !malformed_processor,
    }
}

/// Returns the complete, closed argv for one CPUFreq observation. A partial or
/// oversized topology is intentionally not probed: publishing one CPU's value
/// as the governor of a heterogeneous guest would be a false observation.
pub(super) fn governor_paths(topology: &GuestCpuTopology) -> Option<GovernorPaths> {
    let ids = &topology.logical_cpu_ids;
    if !topology.cpu_ids_complete || ids.is_empty() || ids.len() > MAX_GUEST_CPU_IDS {
        return None;
    }
    let paths = ids
        .iter()
        .map(|id| format!("/sys/devices/system/cpu/cpu{id}/cpufreq/scaling_governor"))
        .collect::<Vec<_>>();
    Some(GovernorPaths(paths))
}

/// Parses the concatenated output of `cat` over every visible CPU's CPUFreq
/// link. Every path must have produced exactly one valid line and every policy
/// must agree; otherwise there is no truthful single governor value to publish.
pub(super) fn parse_uniform_governor(bytes: &[u8], expected_cpus: usize) -> Option<String> {
    if expected_cpus == 0 || expected_cpus > MAX_GUEST_CPU_IDS {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if !text.ends_with('\n') {
        return None;
    }
    let values = text.strip_suffix('\n')?.split('\n').collect::<Vec<_>>();
    if values.len() != expected_cpus || !values.iter().all(|value| valid_text(value)) {
        return None;
    }
    let first = *values.first()?;
    values
        .iter()
        .all(|value| *value == first)
        .then(|| first.to_owned())
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{
        GuestCpuTopology, MAX_GUEST_CPU_IDS, governor_paths, parse_cpuinfo, parse_uniform_governor,
    };

    #[test]
    fn cpuinfo_preserves_a_non_contiguous_guest_cpu_set() -> Result<(), String> {
        let topology = parse_cpuinfo(b"processor : 7\nmodel name : Fixture CPU\nprocessor : 1\n");
        assert_eq!(topology.cpu_model.as_deref(), Some("Fixture CPU"));
        assert_eq!(topology.logical_cpu_ids, [1, 7]);
        let paths = governor_paths(&topology)
            .ok_or_else(|| "bounded non-empty topology was refused".to_owned())?;
        assert_eq!(
            paths.as_slice(),
            [
                "/sys/devices/system/cpu/cpu1/cpufreq/scaling_governor".to_owned(),
                "/sys/devices/system/cpu/cpu7/cpufreq/scaling_governor".to_owned(),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_missing_or_oversized_topology_never_yields_partial_paths() {
        assert_eq!(governor_paths(&GuestCpuTopology::default()), None);
        let malformed = parse_cpuinfo(b"processor: 0\nprocessor: not-a-cpu\n");
        assert_eq!(governor_paths(&malformed), None);
        let duplicate = parse_cpuinfo(b"processor: 7\nprocessor: 7\n");
        assert_eq!(duplicate.logical_cpu_ids, [7]);
        assert!(!duplicate.cpu_ids_complete);
        assert_eq!(governor_paths(&duplicate), None);
        let overflow = parse_cpuinfo(b"processor: 65536\n");
        assert!(overflow.logical_cpu_ids.is_empty());
        assert!(!overflow.cpu_ids_complete);
        let topology = GuestCpuTopology {
            cpu_model: None,
            logical_cpu_ids: (0..=MAX_GUEST_CPU_IDS as u16).collect(),
            cpu_ids_complete: true,
        };
        assert_eq!(governor_paths(&topology), None);
    }

    #[test]
    fn cpu_model_requires_consensus_across_the_guest() {
        let heterogeneous = parse_cpuinfo(
            b"processor: 0\nmodel name: Fast CPU\nprocessor: 1\nmodel name: Slow CPU\n",
        );
        assert_eq!(heterogeneous.cpu_model, None);
        let arm_mismatch = parse_cpuinfo(
            b"processor: 0\nCPU implementer: 0x41\nCPU part: 0xd03\nCPU variant: 0x0\nCPU revision: 1\n\
              processor: 1\nCPU implementer: 0x41\nCPU part: 0xd08\nCPU variant: 0x0\nCPU revision: 1\n",
        );
        assert_eq!(arm_mismatch.cpu_model, None);
    }

    #[test]
    fn a_governor_is_observable_only_when_every_cpu_agrees() {
        assert_eq!(
            parse_uniform_governor(b"schedutil\nschedutil\n", 2),
            Some("schedutil".into())
        );
        for (bytes, count) in [
            (&b"schedutil\npowersave\n"[..], 2),
            (&b"schedutil\n"[..], 2),
            (&b"schedutil"[..], 1),
            (&b"\n"[..], 1),
            (&[0xff, b'\n'][..], 1),
            (&b"sched\x01util\n"[..], 1),
        ] {
            assert_eq!(parse_uniform_governor(bytes, count), None, "{bytes:?}");
        }
    }
}
