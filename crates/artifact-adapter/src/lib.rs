//! Process-local bounded artifacts. No filesystem, process or network operations.
use rust_engineering_application::{ArtifactInput, ArtifactStore, RegistryClock};
use rust_engineering_domain::{
    ArtifactError, ArtifactId, ArtifactMetadata, ArtifactView, ProjectRef,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const CHUNK: usize = 4096;
const MAX_SECRET: usize = 128;
const ID_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct ArtifactLimits {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub global_bytes: usize,
    pub owner_bytes: usize,
    pub global_count: usize,
    pub owner_count: usize,
    pub ttl_seconds: u64,
}
impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            input_bytes: 1024 * 1024,
            output_bytes: 256 * 1024,
            global_bytes: 16 * 1024 * 1024,
            owner_bytes: 1024 * 1024,
            global_count: 256,
            owner_count: 64,
            ttl_seconds: 3600,
        }
    }
}
impl ArtifactLimits {
    fn validate(self) -> Result<(), ArtifactError> {
        if self.input_bytes == 0
            || self.input_bytes > 1024 * 1024
            || self.output_bytes == 0
            || self.output_bytes > 256 * 1024
            || self.output_bytes > self.input_bytes
            || self.global_bytes > 16 * 1024 * 1024
            || self.owner_bytes > 1024 * 1024
            || self.owner_bytes > self.global_bytes
            || self.owner_bytes < self.output_bytes
            || self.global_count == 0
            || self.global_count > 256
            || self.owner_count == 0
            || self.owner_count > 64
            || self.owner_count > self.global_count
            || !(1..=86400).contains(&self.ttl_seconds)
        {
            return Err(ArtifactError::InvalidLimits);
        }
        Ok(())
    }
}
struct Entry {
    metadata: ArtifactMetadata,
    content: Box<[u8]>,
}

pub struct MemoryArtifactStore<C: RegistryClock> {
    clock: C,
    limits: ArtifactLimits,
    secrets: Vec<Vec<u8>>,
    entries: HashMap<ArtifactId, Entry>,
    last_clock: Option<u64>,
    poisoned: bool,
}
impl<C: RegistryClock> MemoryArtifactStore<C> {
    /// Requires a monotonic RegistryClock. A regression clears and permanently
    /// invalidates this instance; the host recovers by constructing a new store
    /// with a trustworthy clock, never by reusing old artifacts.
    pub fn new(
        clock: C,
        limits: ArtifactLimits,
        mut secrets: Vec<Vec<u8>>,
    ) -> Result<Self, ArtifactError> {
        limits.validate()?;
        if secrets.len() > 8 || secrets.iter().any(|s| s.is_empty() || s.len() > MAX_SECRET) {
            return Err(ArtifactError::InvalidSecret);
        }
        for secret in &mut secrets {
            secret.shrink_to_fit();
        }
        secrets.shrink_to_fit();
        Ok(Self {
            clock,
            limits,
            secrets,
            entries: HashMap::new(),
            last_clock: None,
            poisoned: false,
        })
    }
    fn now(&mut self) -> Result<u64, ArtifactError> {
        let now = self.clock.seconds();
        if self.poisoned || self.last_clock.is_some_and(|last| now < last) {
            self.entries.clear();
            self.poisoned = true;
            return Err(ArtifactError::ClockRegression);
        }
        self.last_clock = Some(now);
        Ok(now)
    }
    fn expire(&mut self, now: u64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, e| e.metadata.expires_seconds > now);
        before - self.entries.len()
    }
    fn admit(&self, owner: &ProjectRef) -> Result<(), ArtifactError> {
        let mut bytes = 0usize;
        let mut owner_bytes = 0usize;
        let mut owner_count = 0usize;
        for entry in self.entries.values() {
            bytes = bytes
                .checked_add(entry.content.len())
                .ok_or(ArtifactError::QuotaExceeded)?;
            if &entry.metadata.owner == owner {
                owner_bytes = owner_bytes
                    .checked_add(entry.content.len())
                    .ok_or(ArtifactError::QuotaExceeded)?;
                owner_count += 1;
            }
        }
        // Exclusive &mut capture reserves the maximum draft budget without a second writer.
        // Only successful publication changes usage, so every error rolls the draft back.
        if self.entries.len() >= self.limits.global_count
            || owner_count >= self.limits.owner_count
            || bytes
                .checked_add(self.limits.output_bytes)
                .is_none_or(|n| n > self.limits.global_bytes)
            || owner_bytes
                .checked_add(self.limits.output_bytes)
                .is_none_or(|n| n > self.limits.owner_bytes)
        {
            return Err(ArtifactError::QuotaExceeded);
        }
        Ok(())
    }
    fn capture_with_generator(
        &mut self,
        owner: &ProjectRef,
        input: &mut dyn ArtifactInput,
        generator: &mut impl FnMut() -> Result<[u8; 16], ArtifactError>,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        let now = self.now()?;
        self.expire(now);
        now.checked_add(self.limits.ttl_seconds)
            .ok_or(ArtifactError::ClockOverflow)?;
        self.admit(owner)?;
        let mut selected = None;
        for _ in 0..ID_ATTEMPTS {
            let bytes = generator()?;
            let mut encoded = String::with_capacity(36);
            encoded.push_str("art_");
            const HEX: &[u8; 16] = b"0123456789abcdef";
            for byte in bytes {
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 15)]));
            }
            let id = ArtifactId::try_from(encoded)?;
            if !self.entries.contains_key(&id) {
                selected = Some(id);
                break;
            }
        }
        let id = selected.ok_or(ArtifactError::IdExhausted)?;
        let (content, truncated) = redact(input, self.limits, &self.secrets)?;
        let truncated = truncated || input.truncated();
        let content = content.into_boxed_slice();
        let now = self.now()?;
        self.expire(now);
        let expires_seconds = now
            .checked_add(self.limits.ttl_seconds)
            .ok_or(ArtifactError::ClockOverflow)?;
        let metadata = ArtifactMetadata {
            owner: owner.clone(),
            id: id.clone(),
            sha256: Sha256::digest(&content).into(),
            size_bytes: u32::try_from(content.len()).map_err(|_| ArtifactError::InvalidLimits)?,
            truncated,
            created_seconds: now,
            expires_seconds,
        };
        self.entries.insert(
            id,
            Entry {
                metadata: metadata.clone(),
                content,
            },
        );
        Ok(metadata)
    }
}
impl<C: RegistryClock> ArtifactStore for MemoryArtifactStore<C> {
    fn capture(
        &mut self,
        owner: &ProjectRef,
        input: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        self.capture_with_generator(owner, input, &mut || {
            let mut bytes = [0; 16];
            getrandom::fill(&mut bytes).map_err(|_| ArtifactError::EntropyUnavailable)?;
            Ok(bytes)
        })
    }
    fn read<'a>(
        &'a mut self,
        owner: &ProjectRef,
        id: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        let now = self.now()?;
        self.expire(now);
        let entry = self
            .entries
            .get(id)
            .filter(|e| &e.metadata.owner == owner)
            .ok_or(ArtifactError::NotFound)?;
        Ok(ArtifactView {
            metadata: &entry.metadata,
            content: &entry.content,
        })
    }
    fn remove(&mut self, owner: &ProjectRef, id: &ArtifactId) -> Result<bool, ArtifactError> {
        let now = self.now()?;
        self.expire(now);
        if self
            .entries
            .get(id)
            .is_some_and(|entry| &entry.metadata.owner == owner)
        {
            self.entries.remove(id);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn retain_owners(&mut self, owners: &[ProjectRef]) -> Result<usize, ArtifactError> {
        let now = self.now()?;
        let before = self.entries.len();
        self.expire(now);
        self.entries
            .retain(|_, entry| owners.contains(&entry.metadata.owner));
        Ok(before - self.entries.len())
    }
    fn revoke_owner(&mut self, owner: &ProjectRef) -> Result<usize, ArtifactError> {
        let now = self.now()?;
        self.expire(now);
        let before = self.entries.len();
        self.entries.retain(|_, e| &e.metadata.owner != owner);
        Ok(before - self.entries.len())
    }
    fn cleanup(&mut self) -> Result<usize, ArtifactError> {
        let now = self.now()?;
        Ok(self.expire(now))
    }
}

fn redact(
    input: &mut dyn ArtifactInput,
    limits: ArtifactLimits,
    secrets: &[Vec<u8>],
) -> Result<(Vec<u8>, bool), ArtifactError> {
    // Also defend the private helper used directly by adversarial tests.
    if secrets.iter().any(Vec::is_empty) {
        return Err(ArtifactError::InvalidSecret);
    }
    let keep = secrets.iter().map(Vec::len).max().unwrap_or(1) - 1;
    let mut pending = Vec::with_capacity(CHUNK + MAX_SECRET);
    let mut marked = Vec::with_capacity(CHUNK + MAX_SECRET);
    let mut output = Vec::with_capacity(limits.output_bytes);
    let mut tail = Vec::with_capacity(MAX_SECRET);
    let mut buffer = [0u8; CHUNK];
    let mut consumed = 0usize;
    let mut truncated = false;
    loop {
        if consumed == limits.input_bytes {
            truncated = true;
            break;
        }
        let capacity = CHUNK.min(limits.input_bytes - consumed);
        let n = input
            .read(&mut buffer[..capacity])
            .map_err(|_| ArtifactError::InputFailure)?;
        if n > capacity {
            return Err(ArtifactError::InvalidSourceCount);
        }
        if n == 0 {
            break;
        }
        consumed += n;
        pending.extend_from_slice(&buffer[..n]);
        marked.resize(pending.len(), false);
        for secret in secrets {
            for start in 0..pending.len().saturating_sub(secret.len() - 1) {
                if pending[start..].starts_with(secret) {
                    marked[start..start + secret.len()].fill(true);
                }
            }
        }
        let safe = pending.len().saturating_sub(keep);
        emit(
            &pending,
            &marked,
            safe,
            &mut output,
            &mut tail,
            limits.output_bytes,
            keep,
        );
        if output.len() == limits.output_bytes {
            truncated = true;
            break;
        }
        pending.drain(..safe);
        marked.drain(..safe);
    }
    if output.len() < limits.output_bytes {
        emit(
            &pending,
            &marked,
            pending.len(),
            &mut output,
            &mut tail,
            limits.output_bytes,
            keep,
        );
        if output.len() == limits.output_bytes {
            truncated = true;
        }
    }
    // Examine original emitted bytes, not masked bytes: overlapping matches may have
    // already masked part of the suffix. Also protects an output cutoff mid-prefix.
    for secret in secrets {
        for len in 1..secret.len().min(tail.len() + 1) {
            if tail.ends_with(&secret[..len]) {
                let start = output.len() - len;
                output[start..].fill(b'*');
            }
        }
    }
    Ok((output, truncated))
}
fn emit(
    pending: &[u8],
    marked: &[bool],
    count: usize,
    output: &mut Vec<u8>,
    tail: &mut Vec<u8>,
    cap: usize,
    keep: usize,
) {
    let count = count.min(cap - output.len());
    for (byte, redact) in pending[..count].iter().zip(&marked[..count]) {
        output.push(if *redact { b'*' } else { *byte });
    }
    if keep > 0 {
        if count >= keep {
            tail.clear();
            tail.extend_from_slice(&pending[count - keep..count]);
        } else {
            let remove = (tail.len() + count).saturating_sub(keep);
            tail.drain(..remove);
            tail.extend_from_slice(&pending[..count]);
        }
    }
}

#[cfg(test)]
mod tests;
