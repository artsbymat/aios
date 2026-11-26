//! Contiguous slab-backed UTF-8 text corpus designed for huge streaming lists (e.g. `find / | aios dmenu`).
//!
//! Why this exists:
//! Storing 1,000,000+ paths as `Vec<String>` creates millions of tiny heap allocations across
//! threads (`aios-ipc-dmenu-stream` -> UI main thread -> `Arc<[String]>` deep clone), which
//! Linux glibc retains indefinitely inside fragmented `brk` / per-thread arenas even after
//! `DmenuWindow` drops.
//!
//! [`PackedTextCorpus`] packs item text and 4-byte end offsets into unified contiguous slabs.
//! Small lists (`<= 64 KiB`) stay tiny, while large lists allocate `33 MiB` slabs — strictly
//! above glibc's `32 MiB` `DEFAULT_MMAP_THRESHOLD_MAX` — so the OS backs them with lazy
//! demand-paged `mmap()` and reclaims 100% of their physical RAM immediately via `munmap()`
//! when the corpus drops.

use std::sync::Arc;

/// Initial compact slab size for small `dmenu` lists (no wasted virtual address space).
const SMALL_SLAB_BYTES: usize = 64 * 1024;

/// Bulk slab size strictly exceeding glibc's 64-bit `DEFAULT_MMAP_THRESHOLD_MAX` (32 MiB),
/// guaranteeing direct `mmap()` allocation and immediate `munmap()` return to the OS on `Drop`.
const BULK_SLAB_BYTES: usize = 33 * 1024 * 1024;

/// Minimum used bytes in an active bulk slab before sealing it directly for zero-copy search snapshots.
const SNAPSHOT_SEAL_THRESHOLD_BYTES: usize = 512 * 1024;

const OFFSET_SIZE: usize = std::mem::size_of::<u32>();

/// One contiguous buffer holding UTF-8 strings from the front (`0..text_len`) and
/// 4-byte native-endian end offsets growing backward from the tail (`cap - (i+1)*4`).
///
/// Unused middle bytes in a bulk `vec![0u8; 33 MiB]` remain untouched zero-pages and
/// consume 0 bytes of physical RSS on Linux.
#[derive(Debug, Clone)]
pub(crate) struct PackedTextSlab {
    buf: Vec<u8>,
    text_len: usize,
    count: usize,
    is_bulk: bool,
}

impl PackedTextSlab {
    fn new_small() -> Self {
        Self {
            buf: vec![0u8; SMALL_SLAB_BYTES],
            text_len: 0,
            count: 0,
            is_bulk: false,
        }
    }

    fn new_bulk(min_payload_bytes: usize) -> Self {
        let cap = min_payload_bytes
            .saturating_add(OFFSET_SIZE)
            .max(BULK_SLAB_BYTES);
        Self {
            buf: vec![0u8; cap],
            text_len: 0,
            count: 0,
            is_bulk: true,
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.count
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    fn used_bytes(&self) -> usize {
        self.text_len + self.count * OFFSET_SIZE
    }

    fn try_push(&mut self, text: &str) -> bool {
        let bytes = text.as_bytes();
        let Some(next_text_len) = self.text_len.checked_add(bytes.len()) else {
            return false;
        };
        if next_text_len > u32::MAX as usize {
            return false;
        }
        let next_meta = (self.count + 1).saturating_mul(OFFSET_SIZE);
        let Some(total_needed) = next_text_len.checked_add(next_meta) else {
            return false;
        };
        if total_needed > self.buf.len() {
            return false;
        }

        self.buf[self.text_len..next_text_len].copy_from_slice(bytes);
        self.text_len = next_text_len;
        self.count += 1;

        let meta_pos = self.buf.len() - self.count * OFFSET_SIZE;
        self.buf[meta_pos..meta_pos + OFFSET_SIZE]
            .copy_from_slice(&(next_text_len as u32).to_ne_bytes());
        true
    }

    /// Creates a tightly-sized immutable clone containing only the occupied bytes + offsets.
    fn compact_clone(&self) -> Self {
        let meta_bytes = self.count * OFFSET_SIZE;
        let total = self.text_len + meta_bytes;
        let mut buf = vec![0u8; total];
        buf[..self.text_len].copy_from_slice(&self.buf[..self.text_len]);
        let src_meta_start = self.buf.len() - meta_bytes;
        buf[self.text_len..].copy_from_slice(&self.buf[src_meta_start..]);
        Self {
            buf,
            text_len: self.text_len,
            count: self.count,
            is_bulk: false,
        }
    }

    #[inline]
    pub(crate) fn get(&self, local_idx: usize) -> Option<&str> {
        if local_idx >= self.count {
            return None;
        }
        let cap = self.buf.len();
        let end_pos = cap - (local_idx + 1) * OFFSET_SIZE;
        let end = u32::from_ne_bytes(
            self.buf[end_pos..end_pos + OFFSET_SIZE]
                .try_into()
                .expect("4-byte offset slice"),
        ) as usize;
        let start = if local_idx == 0 {
            0
        } else {
            let prev_pos = cap - local_idx * OFFSET_SIZE;
            u32::from_ne_bytes(
                self.buf[prev_pos..prev_pos + OFFSET_SIZE]
                    .try_into()
                    .expect("4-byte offset slice"),
            ) as usize
        };
        std::str::from_utf8(&self.buf[start..end]).ok()
    }
}

/// Immutable, cheaply cloneable (`O(slabs)` `Arc` bumps) snapshot of a [`PackedTextCorpus`]
/// suitable for zero-copy parallel scoring on background Rayon threads.
#[derive(Debug, Clone, Default)]
pub struct PackedTextSnapshot {
    slabs: Vec<Arc<PackedTextSlab>>,
    /// Prefix sums of item counts: `slab_ends[i]` is the exclusive global end index of `slabs[i]`.
    slab_ends: Vec<usize>,
    total_len: usize,
}

impl PackedTextSnapshot {
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.total_len
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Resolves item `index` in `O(log slabs)` (typically 1-4 slabs per million items) with zero allocation.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&str> {
        if index >= self.total_len {
            return None;
        }
        let slab_idx = self.slab_ends.partition_point(|&end| end <= index);
        let slab_start = if slab_idx == 0 {
            0
        } else {
            self.slab_ends[slab_idx - 1]
        };
        self.slabs[slab_idx].get(index - slab_start)
    }
}

/// Append-only packed UTF-8 text corpus with zero per-item heap allocations and instant OS reclamation.
#[derive(Debug, Clone, Default)]
pub struct PackedTextCorpus {
    sealed: Vec<Arc<PackedTextSlab>>,
    sealed_ends: Vec<usize>,
    sealed_len: usize,
    active: Option<PackedTextSlab>,
    total_len: usize,
}

impl PackedTextCorpus {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a packed corpus directly from an iterator of string slices or owned `String`s.
    pub fn from_iter_strs<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut corpus = Self::new();
        for item in iter {
            corpus.push(item.as_ref());
        }
        corpus
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.total_len
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Appends one UTF-8 item into the current contiguous slab without per-item `malloc()`.
    pub fn push(&mut self, text: &str) {
        if let Some(active) = self.active.as_mut()
            && active.try_push(text)
        {
            self.total_len += 1;
            return;
        }

        // Current slab is absent or full: seal it (if non-empty) and allocate the next slab.
        let had_any_slab = self.active.is_some() || !self.sealed.is_empty();
        if let Some(full) = self.active.take()
            && !full.is_empty()
        {
            self.seal_slab(full);
        }

        let mut next = if !had_any_slab && text.len() + OFFSET_SIZE <= SMALL_SLAB_BYTES {
            PackedTextSlab::new_small()
        } else {
            PackedTextSlab::new_bulk(text.len())
        };
        let pushed = next.try_push(text);
        debug_assert!(pushed, "freshly sized slab must fit item");
        self.active = Some(next);
        self.total_len += 1;
    }

    /// Appends a batch of strings while copying them into contiguous slabs and dropping input `String`s immediately.
    pub fn extend_strs<I, S>(&mut self, iter: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for item in iter {
            self.push(item.as_ref());
        }
    }

    /// Returns the UTF-8 text at `index` in `O(1)` amortized time with zero allocation.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&str> {
        if index >= self.total_len {
            return None;
        }
        if index >= self.sealed_len {
            return self
                .active
                .as_ref()
                .and_then(|slab| slab.get(index - self.sealed_len));
        }
        let slab_idx = self.sealed_ends.partition_point(|&end| end <= index);
        let slab_start = if slab_idx == 0 {
            0
        } else {
            self.sealed_ends[slab_idx - 1]
        };
        self.sealed[slab_idx].get(index - slab_start)
    }

    /// Produces an immutable [`PackedTextSnapshot`] for background parallel search with near-zero copy.
    ///
    /// Large active slabs (`>= 512 KiB` used) are sealed in-place into an `Arc` with zero bytes copied
    /// (untouched middle zero-pages never fault physical RAM and `munmap()` cleanly on `Drop`).
    /// Small active slabs (`< 512 KiB` used) are compactly cloned so rapid typing during small streams
    /// never creates excessive virtual mappings.
    #[must_use]
    pub fn snapshot(&mut self) -> PackedTextSnapshot {
        let mut slabs = self.sealed.clone();
        let mut slab_ends = self.sealed_ends.clone();

        if let Some(active) = self.active.as_ref()
            && !active.is_empty()
        {
            if active.is_bulk && active.used_bytes() >= SNAPSHOT_SEAL_THRESHOLD_BYTES {
                let full = self.active.take().expect("active slab present");
                self.seal_slab(full);
                slabs = self.sealed.clone();
                slab_ends = self.sealed_ends.clone();
            } else {
                let compact = Arc::new(active.compact_clone());
                slabs.push(compact);
                slab_ends.push(self.total_len);
            }
        }

        PackedTextSnapshot {
            slabs,
            slab_ends,
            total_len: self.total_len,
        }
    }

    /// Seals any active slab once streaming reaches EOF so subsequent snapshots are pure `O(slabs)` `Arc` clones.
    pub fn seal_active(&mut self) {
        if let Some(active) = self.active.take()
            && !active.is_empty()
        {
            let finalized = if !active.is_bulk {
                active.compact_clone()
            } else {
                active
            };
            self.seal_slab(finalized);
        }
    }

    /// Releases all slabs immediately (triggering immediate `munmap()` for every bulk slab).
    pub fn clear(&mut self) {
        self.sealed.clear();
        self.sealed.shrink_to_fit();
        self.sealed_ends.clear();
        self.sealed_ends.shrink_to_fit();
        self.sealed_len = 0;
        self.active = None;
        self.total_len = 0;
    }

    fn seal_slab(&mut self, slab: PackedTextSlab) {
        self.sealed_len += slab.len();
        self.sealed.push(Arc::new(slab));
        self.sealed_ends.push(self.sealed_len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_corpus_round_trips_small_and_multi_slab_entries() {
        let mut corpus = PackedTextCorpus::new();
        assert!(corpus.is_empty());

        // Exceed SMALL_SLAB_BYTES (64 KiB) to exercise transition from small slab to bulk slab.
        let count = 3_000usize;
        for i in 0..count {
            corpus.push(&format!("/home/rahmat/projects/aios/crates/file_{i:05}.rs"));
        }

        assert_eq!(corpus.len(), count);
        assert_eq!(
            corpus.get(0),
            Some("/home/rahmat/projects/aios/crates/file_00000.rs")
        );
        assert_eq!(
            corpus.get(count - 1),
            Some("/home/rahmat/projects/aios/crates/file_02999.rs")
        );
        assert_eq!(corpus.get(count), None);

        let snap = corpus.snapshot();
        assert_eq!(snap.len(), count);
        for i in [0, 1, 500, 1499, 1500, count - 1] {
            assert_eq!(snap.get(i), corpus.get(i));
        }

        corpus.clear();
        assert!(corpus.is_empty());
        assert_eq!(corpus.get(0), None);
    }

    #[test]
    fn packed_search_session_and_parallel_match_large_corpus_and_reclaim_memory() {
        use crate::{FrecencyMap, PackedSearchSession, SearchConfig, search_packed_parallel};
        use std::sync::Arc;
        use std::time::Duration;

        #[cfg(target_os = "linux")]
        fn private_dirty_kb() -> Option<u64> {
            let raw = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
            raw.lines()
                .find_map(|line| line.strip_prefix("Private_Dirty:"))
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|v| v.parse::<u64>().ok())
        }

        #[cfg(target_os = "linux")]
        let baseline_kb = private_dirty_kb();

        // Run 2 consecutive large-corpus cycles (~500k paths each, >33 MiB per cycle) to verify
        // glibc dynamic MMAP_THRESHOLD does not pin released slabs on subsequent runs.
        for cycle in 0..2 {
            let mut corpus = PackedTextCorpus::new();
            for i in 0..500_000usize {
                corpus.push(&format!(
                    "/home/rahmat/work/aios/cycle_{cycle}/dir_{:03}/entry_{i:06}.rs",
                    i % 256
                ));
            }
            corpus.seal_active();
            let snap = corpus.snapshot();

            let cfg = SearchConfig {
                limit: 50,
                inline_budget: Duration::from_micros(500),
                inline_max_items: 512,
                ..SearchConfig::default()
            };
            let frecency = Arc::new(FrecencyMap::new());

            let par = search_packed_parallel(&snap, "entry_499999", &frecency, 0, &cfg, None)
                .expect("parallel search completes");
            assert_eq!(par.total_matched, 1);
            assert_eq!(par.matches[0].item_index, 499_999);

            let session = PackedSearchSession::new(cfg);
            let handle = session.search(snap, "entry_250000", Arc::clone(&frecency), 0);
            let mut final_snap = handle.initial;
            if let Some(rx) = handle.updates {
                while let Ok(next) = rx.recv() {
                    let done = next.is_complete;
                    final_snap = next;
                    if done {
                        break;
                    }
                }
            }
            assert!(final_snap.is_complete);
            assert_eq!(final_snap.matches[0].item_index, 250_000);

            drop(session);
            corpus.clear();
        }

        #[cfg(target_os = "linux")]
        if let (Some(before), Some(after)) = (baseline_kb, private_dirty_kb()) {
            // Two 500k-path cycles touch >70 MiB of corpus data; after drop, residual growth
            // must stay well under 24 MiB (proving bulk 33 MiB slabs were munmap()'d cleanly).
            let delta_kb = after.saturating_sub(before);
            assert!(
                delta_kb < 24 * 1024,
                "expected packed corpus slabs to munmap after drop; residual Private_Dirty delta = {delta_kb} kB"
            );
        }
    }
}
