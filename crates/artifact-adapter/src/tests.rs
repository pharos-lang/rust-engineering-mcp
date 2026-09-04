#![allow(clippy::unwrap_used)]
use super::*;
use std::{cell::Cell, rc::Rc};
#[derive(Clone, Default)]
struct Clock(Rc<Cell<u64>>);
impl RegistryClock for Clock {
    fn seconds(&self) -> u64 {
        self.0.get()
    }
}
struct Source {
    bytes: Vec<u8>,
    offset: usize,
    chunk: usize,
    reads: usize,
    fail_after: Option<usize>,
}
impl Source {
    fn new(bytes: &[u8], chunk: usize) -> Self {
        Self {
            bytes: bytes.to_vec(),
            offset: 0,
            chunk,
            reads: 0,
            fail_after: None,
        }
    }
}
impl ArtifactInput for Source {
    fn read(&mut self, out: &mut [u8]) -> Result<usize, ArtifactError> {
        self.reads += 1;
        if self.fail_after.is_some_and(|n| self.offset >= n) {
            return Err(ArtifactError::InputFailure);
        }
        let n = out
            .len()
            .min(self.chunk)
            .min(self.bytes.len() - self.offset);
        out[..n].copy_from_slice(&self.bytes[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}
fn owner(n: u8) -> ProjectRef {
    format!("prj_{n:032x}").parse().unwrap()
}
fn limits() -> ArtifactLimits {
    ArtifactLimits {
        input_bytes: 16384,
        output_bytes: 8192,
        global_bytes: 32768,
        owner_bytes: 16384,
        global_count: 8,
        owner_count: 4,
        ttl_seconds: 10,
    }
}
fn store(limits: ArtifactLimits, secrets: &[&[u8]]) -> MemoryArtifactStore<Clock> {
    MemoryArtifactStore::new(
        Clock::default(),
        limits,
        secrets.iter().map(|s| s.to_vec()).collect(),
    )
    .unwrap()
}
fn captured(
    store: &mut MemoryArtifactStore<Clock>,
    bytes: &[u8],
    chunk: usize,
) -> (ArtifactMetadata, Vec<u8>) {
    let meta = store
        .capture(&owner(1), &mut Source::new(bytes, chunk))
        .unwrap();
    let content = store.read(&owner(1), &meta.id).unwrap().content.to_vec();
    (meta, content)
}
#[test]
fn overlapping_nested_and_adjacent_matches_all_chunk_sizes() {
    for chunk in 1..=12 {
        for (secrets, input, expected) in [
            (
                vec![&b"abc"[..], &b"bcdef"[..]],
                &b"!abcdef!"[..],
                &b"!******!"[..],
            ),
            (
                vec![&b"abc"[..], &b"abcdef"[..]],
                &b"abcdefabc"[..],
                &b"*********"[..],
            ),
            (vec![&b"aba"[..]], &b"ababa"[..], &b"*****"[..]),
        ] {
            assert_eq!(
                captured(&mut store(limits(), &secrets), input, chunk).1,
                expected
            );
        }
    }
}
#[test]
fn matches_cross_4096_boundary_and_keep_flags() {
    let secret = vec![b'Q'; 128];
    let mut bytes = vec![b'!'; 4095];
    bytes.extend_from_slice(&secret);
    bytes.push(b'!');
    let mut expected = vec![b'!'; 4095];
    expected.extend_from_slice(&[b'*'; 128]);
    expected.push(b'!');
    for chunk in [1, 127, 4096] {
        assert_eq!(
            captured(&mut store(limits(), &[&secret]), &bytes, chunk).1,
            expected
        );
    }
}
#[test]
fn eof_and_both_budget_suffixes_are_conservative() {
    assert_eq!(
        captured(&mut store(limits(), &[b"abcdef", b"bcdefg"]), b"!abc", 1).1,
        b"!***"
    );
    let mut l = limits();
    l.input_bytes = 4;
    l.output_bytes = 4;
    let (m, b) = captured(&mut store(l, &[b"abcdef"]), b"!abcZZZ", 1);
    assert_eq!(b, b"!***");
    assert!(m.truncated);
    l.input_bytes = 20;
    l.output_bytes = 3;
    let (m, b) = captured(&mut store(l, &[b"abcdef"]), b"!abZmore", 20);
    assert_eq!(b, b"!**");
    assert!(m.truncated);
    let (m, b) = captured(&mut store(l, &[]), b"123", 20);
    assert_eq!(b, b"123");
    assert!(m.truncated);
}
#[test]
fn binary_patterns_and_stored_hash_metadata() {
    let (m, b) = captured(&mut store(limits(), &[&[0, 255, 1]]), &[9, 0, 255, 1, 8], 1);
    assert_eq!(b, [9, b'*', b'*', b'*', 8]);
    assert_eq!(m.size_bytes, 5);
    assert!(!m.truncated);
    let expected: [u8; 32] = Sha256::digest(&b).into();
    assert_eq!(m.sha256, expected);
    assert_eq!(m.created_seconds, 0);
    assert_eq!(m.expires_seconds, 10);
}
#[test]
fn malformed_source_and_partial_error_roll_back() {
    struct Bad;
    impl ArtifactInput for Bad {
        fn read(&mut self, b: &mut [u8]) -> Result<usize, ArtifactError> {
            Ok(b.len() + 1)
        }
    }
    let mut s = store(limits(), &[]);
    assert_eq!(
        s.capture(&owner(1), &mut Bad),
        Err(ArtifactError::InvalidSourceCount)
    );
    let mut input = Source::new(&vec![b'x'; 8192], 4096);
    input.fail_after = Some(4096);
    assert_eq!(
        s.capture(&owner(1), &mut input),
        Err(ArtifactError::InputFailure)
    );
    assert!(s.entries.is_empty());
    assert_eq!(captured(&mut s, b"okay", 1).1, b"okay");
}
#[test]
fn endless_source_bounded_no_extra_probe() {
    struct Endless(usize, usize);
    impl ArtifactInput for Endless {
        fn read(&mut self, b: &mut [u8]) -> Result<usize, ArtifactError> {
            self.0 += 1;
            self.1 += b.len();
            b.fill(b'x');
            Ok(b.len())
        }
    }
    let mut l = limits();
    l.input_bytes = 17;
    l.output_bytes = 17;
    let mut s = store(l, &[b"longsecret"]);
    let mut input = Endless(0, 0);
    let m = s.capture(&owner(1), &mut input).unwrap();
    assert!(m.truncated);
    assert_eq!(m.size_bytes, 17);
    assert_eq!((input.0, input.1), (1, 17));
    l.input_bytes = 16384;
    l.output_bytes = 8;
    let mut s = store(l, &[]);
    let mut input = Endless(0, 0);
    assert!(s.capture(&owner(1), &mut input).unwrap().truncated);
    assert_eq!((input.0, input.1), (1, 4096));
}
#[test]
fn maximum_budget_reserved_before_read_and_released_on_revoke() {
    let mut l = limits();
    l.output_bytes = 8;
    l.owner_bytes = 8;
    l.global_bytes = 16;
    let mut s = store(l, &[]);
    captured(&mut s, b"x", 1);
    let mut source = Source::new(b"", 1);
    assert_eq!(
        s.capture(&owner(1), &mut source),
        Err(ArtifactError::QuotaExceeded)
    );
    assert_eq!(source.reads, 0);
    let second = s.capture(&owner(2), &mut Source::new(b"y", 1)).unwrap();
    assert_eq!(s.revoke_owner(&owner(1)).unwrap(), 1);
    assert_eq!(captured(&mut s, b"12345678", 8).1, b"12345678");
    assert_eq!(s.read(&owner(2), &second.id).unwrap().content, b"y");
    l.global_bytes = 8;
    let mut s = store(l, &[]);
    captured(&mut s, b"x", 1);
    assert_eq!(
        s.capture(&owner(2), &mut source),
        Err(ArtifactError::QuotaExceeded)
    );
}
#[test]
fn empty_artifacts_consume_counts_and_ownership_is_not_an_oracle() {
    let mut l = limits();
    l.owner_count = 1;
    l.global_count = 2;
    let mut s = store(l, &[]);
    let (m, b) = captured(&mut s, b"", 1);
    assert!(b.is_empty());
    assert_eq!(
        s.capture(&owner(1), &mut Source::new(b"", 1)),
        Err(ArtifactError::QuotaExceeded)
    );
    s.capture(&owner(2), &mut Source::new(b"", 1)).unwrap();
    assert_eq!(
        s.capture(&owner(3), &mut Source::new(b"", 1)),
        Err(ArtifactError::QuotaExceeded)
    );
    assert_eq!(
        s.read(&owner(2), &m.id).err(),
        Some(ArtifactError::NotFound)
    );
    let missing = "art_ffffffffffffffffffffffffffffffff".parse().unwrap();
    assert_eq!(
        s.read(&owner(1), &missing).err(),
        Some(ArtifactError::NotFound)
    );
}
#[test]
fn expiry_at_boundary_and_clock_regression_poison() {
    let mut s = store(limits(), &[]);
    let (m, _) = captured(&mut s, b"data", 1);
    s.clock.0.set(9);
    assert!(s.read(&owner(1), &m.id).is_ok());
    s.clock.0.set(10);
    assert_eq!(
        s.read(&owner(1), &m.id).err(),
        Some(ArtifactError::NotFound)
    );
    let (m, _) = captured(&mut s, b"data", 1);
    s.clock.0.set(9);
    assert_eq!(
        s.read(&owner(1), &m.id).err(),
        Some(ArtifactError::ClockRegression)
    );
    assert!(s.entries.is_empty());
    s.clock.0.set(20);
    assert_eq!(s.cleanup(), Err(ArtifactError::ClockRegression));
    assert_eq!(
        s.capture(&owner(1), &mut Source::new(b"", 1)),
        Err(ArtifactError::ClockRegression)
    );
}
#[test]
fn expiry_frees_admission_and_expiry_overflow_rejects_without_input() {
    let mut l = limits();
    l.owner_count = 1;
    let mut s = store(l, &[]);
    captured(&mut s, b"", 1);
    s.clock.0.set(10);
    captured(&mut s, b"", 1);
    s.clock.0.set(20);
    assert_eq!(s.cleanup().unwrap(), 1);
    s.clock.0.set(u64::MAX);
    let mut source = Source::new(b"", 1);
    assert_eq!(
        s.capture(&owner(1), &mut source),
        Err(ArtifactError::ClockOverflow)
    );
    assert_eq!(source.reads, 0);
}
#[test]
fn entropy_and_eight_collisions_fail_before_input_without_overwrite() {
    let mut s = store(limits(), &[]);
    let mut source = Source::new(b"", 1);
    assert_eq!(
        s.capture_with_generator(&owner(1), &mut source, &mut || Err(
            ArtifactError::EntropyUnavailable
        )),
        Err(ArtifactError::EntropyUnavailable)
    );
    assert_eq!(source.reads, 0);
    let m = s
        .capture_with_generator(
            &owner(1),
            &mut Source::new(b"first", 1),
            &mut || Ok([1; 16]),
        )
        .unwrap();
    let mut attempts = 0;
    assert_eq!(
        s.capture_with_generator(&owner(2), &mut source, &mut || {
            attempts += 1;
            Ok([1; 16])
        }),
        Err(ArtifactError::IdExhausted)
    );
    assert_eq!(attempts, 8);
    assert_eq!(source.reads, 0);
    assert_eq!(s.read(&owner(1), &m.id).unwrap().content, b"first");
    let next = s
        .capture_with_generator(&owner(2), &mut source, &mut || Ok([2; 16]))
        .unwrap();
    assert_ne!(next.id, m.id);
}
#[test]
fn invalid_limits_and_secret_configuration() {
    let base = ArtifactLimits::default();
    let invalid = [
        ArtifactLimits {
            input_bytes: 0,
            ..base
        },
        ArtifactLimits {
            input_bytes: 1024 * 1024 + 1,
            ..base
        },
        ArtifactLimits {
            output_bytes: 0,
            ..base
        },
        ArtifactLimits {
            output_bytes: 256 * 1024 + 1,
            ..base
        },
        ArtifactLimits {
            input_bytes: 1,
            ..base
        },
        ArtifactLimits {
            global_bytes: 16 * 1024 * 1024 + 1,
            ..base
        },
        ArtifactLimits {
            owner_bytes: 1024 * 1024 + 1,
            ..base
        },
        ArtifactLimits {
            owner_bytes: 1,
            ..base
        },
        ArtifactLimits {
            global_count: 0,
            ..base
        },
        ArtifactLimits {
            global_count: 257,
            ..base
        },
        ArtifactLimits {
            owner_count: 0,
            ..base
        },
        ArtifactLimits {
            owner_count: 65,
            ..base
        },
        ArtifactLimits {
            global_count: 1,
            ..base
        },
        ArtifactLimits {
            ttl_seconds: 0,
            ..base
        },
        ArtifactLimits {
            ttl_seconds: 86401,
            ..base
        },
    ];
    for l in invalid {
        assert_eq!(
            MemoryArtifactStore::new(Clock::default(), l, vec![]).err(),
            Some(ArtifactError::InvalidLimits)
        );
    }
    for secrets in [vec![vec![]], vec![vec![1; 129]], vec![vec![1]; 9]] {
        assert_eq!(
            MemoryArtifactStore::new(Clock::default(), base, secrets).err(),
            Some(ArtifactError::InvalidSecret)
        );
    }
    assert!(
        MemoryArtifactStore::new(
            Clock::default(),
            ArtifactLimits {
                ttl_seconds: 86400,
                ..base
            },
            vec![vec![1; 128]; 8]
        )
        .is_ok()
    );
}

#[test]
fn streaming_matches_independent_whole_buffer_oracle_at_all_short_cuts() {
    let pattern_sets = [
        vec![
            b"aba".to_vec(),
            b"bab".to_vec(),
            b"abab".to_vec(),
            b"aab".to_vec(),
        ],
        vec![b"a".to_vec(), b"bab".to_vec(), b"bbaababa".to_vec()],
    ];
    let mut combinations = 0usize;
    for secrets in pattern_sets {
        for len in 0..=8 {
            for bits in 0..(1usize << len) {
                let raw: Vec<u8> = (0..len)
                    .map(|i| if bits & (1 << i) == 0 { b'a' } else { b'b' })
                    .collect();
                for input_cap in 1..=9 {
                    // Bytes beyond the input budget cannot inform the oracle.
                    let visible = &raw[..raw.len().min(input_cap)];
                    for output_cap in 1..=input_cap {
                        let n = visible.len().min(output_cap);
                        let emitted = &visible[..n];
                        let mut expected = emitted.to_vec();
                        for (position, byte) in expected.iter_mut().enumerate() {
                            let full_match = secrets.iter().any(|secret| {
                                visible
                                    .windows(secret.len())
                                    .enumerate()
                                    .any(|(start, window)| {
                                        window == secret
                                            && (start..start + secret.len()).contains(&position)
                                    })
                            });
                            // ADR028 deliberately masks proper prefixes at the emitted
                            // cut, including true EOF and output-budget boundaries.
                            let prefix = secrets.iter().any(|secret| {
                                (1..secret.len().min(n + 1)).any(|length| {
                                    emitted.ends_with(&secret[..length]) && position >= n - length
                                })
                            });
                            if full_match || prefix {
                                *byte = b'*';
                            }
                        }
                        for chunk in 1..=5 {
                            let mut l = limits();
                            l.input_bytes = input_cap;
                            l.output_bytes = output_cap;
                            let actual = redact(&mut Source::new(&raw, chunk), l, &secrets)
                                .unwrap()
                                .0;
                            assert_eq!(
                                actual, expected,
                                "raw={raw:?} input_cap={input_cap} output_cap={output_cap} chunk={chunk} secrets={secrets:?}"
                            );
                            combinations += 1;
                        }
                    }
                }
            }
        }
    }
    // 2 pattern sets × 511 binary inputs × 45 valid cap pairs × 5 chunks.
    assert_eq!(combinations, 229_950);
}

#[test]
fn exact_input_cap_is_truncated_without_an_extra_eof_read() {
    for chunk in 1..=5 {
        let mut l = limits();
        l.input_bytes = 4;
        l.output_bytes = 4;
        let mut source = Source::new(b"!abc", chunk);
        // Any read after all four bytes would fail, even an EOF probe.
        source.fail_after = Some(4);
        let (content, truncated) = redact(&mut source, l, &[b"abcdef".to_vec()]).unwrap();
        assert_eq!(content, b"!***");
        assert!(truncated);
        assert_eq!(source.offset, 4);
        assert_eq!(source.reads, 4usize.div_ceil(chunk));
    }
}

#[test]
fn private_redact_rejects_empty_secret_before_reading_input() {
    let mut source = Source::new(b"data", 1);
    assert_eq!(
        redact(&mut source, limits(), &[vec![]]),
        Err(ArtifactError::InvalidSecret)
    );
    assert_eq!(source.reads, 0);
}

#[test]
fn clock_changes_during_capture_do_not_publish_draft() {
    struct ChangesClock {
        clock: Clock,
        next: u64,
    }
    impl ArtifactInput for ChangesClock {
        fn read(&mut self, _: &mut [u8]) -> Result<usize, ArtifactError> {
            self.clock.0.set(self.next);
            Ok(0)
        }
    }
    for (next, error) in [
        (0, ArtifactError::ClockRegression),
        (u64::MAX, ArtifactError::ClockOverflow),
    ] {
        let mut s = store(limits(), &[]);
        s.clock.0.set(10);
        let mut source = ChangesClock {
            clock: s.clock.clone(),
            next,
        };
        assert_eq!(s.capture(&owner(1), &mut source), Err(error));
        assert!(s.entries.is_empty());
    }
}

#[test]
fn remove_is_owner_bound_idempotent_and_releases_capacity() {
    let mut bounded = limits();
    bounded.global_count = 1;
    bounded.owner_count = 1;
    let mut store = store(bounded, &[]);
    let metadata = store
        .capture(&owner(1), &mut Source::new(b"retained", 4))
        .unwrap();
    assert!(!store.remove(&owner(2), &metadata.id).unwrap());
    assert_eq!(
        store.read(&owner(1), &metadata.id).unwrap().content,
        b"retained"
    );
    assert_eq!(
        store.capture(&owner(1), &mut Source::new(b"next", 4)),
        Err(ArtifactError::QuotaExceeded)
    );
    assert!(store.remove(&owner(1), &metadata.id).unwrap());
    assert!(!store.remove(&owner(1), &metadata.id).unwrap());
    assert_eq!(
        store.read(&owner(1), &metadata.id).err(),
        Some(ArtifactError::NotFound)
    );
    assert!(
        store
            .capture(&owner(1), &mut Source::new(b"next", 4))
            .is_ok()
    );
}

#[test]
fn retain_live_owners_frees_dead_capacity_without_evicting_live_logs() {
    let mut bounded = limits();
    bounded.global_count = 2;
    bounded.owner_count = 2;
    let mut s = store(bounded, &[]);
    let live = s.capture(&owner(1), &mut Source::new(b"live", 4)).unwrap();
    let dead = s.capture(&owner(2), &mut Source::new(b"dead", 4)).unwrap();
    assert_eq!(
        s.capture(&owner(3), &mut Source::new(b"next", 4)),
        Err(ArtifactError::QuotaExceeded)
    );
    assert_eq!(s.retain_owners(&[owner(1)]).unwrap(), 1);
    assert_eq!(s.read(&owner(1), &live.id).unwrap().content, b"live");
    assert_eq!(
        s.read(&owner(2), &dead.id).err(),
        Some(ArtifactError::NotFound)
    );
    assert!(s.capture(&owner(3), &mut Source::new(b"next", 4)).is_ok());
    assert_eq!(s.retain_owners(&[owner(1), owner(3)]).unwrap(), 0);
    s.clock.0.set(10);
    assert_eq!(s.retain_owners(&[owner(1), owner(3)]).unwrap(), 2);
}

#[test]
fn upstream_truncation_is_preserved_and_hashes_only_stored_redacted_bytes() {
    struct Lost(Source);
    impl ArtifactInput for Lost {
        fn read(&mut self, out: &mut [u8]) -> Result<usize, ArtifactError> {
            self.0.read(out)
        }
        fn truncated(&self) -> bool {
            true
        }
    }
    let mut s = store(limits(), &[b"secret"]);
    let metadata = s
        .capture(&owner(1), &mut Lost(Source::new(b"short secret", 3)))
        .unwrap();
    let view = s.read(&owner(1), &metadata.id).unwrap();
    assert!(metadata.truncated);
    assert!(!view.content.windows(6).any(|window| window == b"secret"));
    let hash: [u8; 32] = Sha256::digest(view.content).into();
    assert_eq!(metadata.sha256, hash);
    assert_eq!(metadata.size_bytes as usize, view.content.len());
}
