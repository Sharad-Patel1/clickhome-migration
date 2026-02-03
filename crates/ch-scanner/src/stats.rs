//! Scan statistics with atomic counters.
//!
//! This module provides [`ScanStats`] for tracking scan progress and
//! [`StatsSnapshot`] for point-in-time statistics views.
//!
//! # Thread Safety
//!
//! All counters use [`AtomicU64`] with [`Relaxed`](std::sync::atomic::Ordering::Relaxed)
//! ordering for maximum performance. Statistics are for informational purposes
//! and don't require strict ordering guarantees.
//!
//! # Examples
//!
//! ```
//! use ch_scanner::ScanStats;
//!
//! let stats = ScanStats::new();
//!
//! // Increment counters during scanning
//! stats.increment_total();
//! stats.increment_legacy();
//!
//! // Get a snapshot for display
//! let snapshot = stats.snapshot();
//! println!("Scanned {} files, {} legacy", snapshot.total, snapshot.legacy);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Atomic counters for scan statistics.
///
/// Uses relaxed atomic ordering for maximum performance. These statistics
/// are for informational/display purposes and don't require strict ordering.
///
/// # Usage
///
/// Create with [`new()`](Self::new), increment during scanning, and take
/// snapshots with [`snapshot()`](Self::snapshot) for display or reporting.
///
/// # Examples
///
/// ```
/// use ch_scanner::ScanStats;
///
/// let stats = ScanStats::new();
///
/// // During parallel scanning
/// stats.increment_total();
/// stats.increment_legacy();
///
/// // For display
/// let snap = stats.snapshot();
/// println!("Progress: {:.1}%", snap.progress_percent());
/// ```
#[derive(Debug, Default)]
pub struct ScanStats {
    /// Total number of files scanned.
    total: AtomicU64,
    /// Number of files with only legacy imports.
    legacy: AtomicU64,
    /// Number of files with only migrated imports.
    migrated: AtomicU64,
    /// Number of files with both legacy and migrated imports.
    partial: AtomicU64,
    /// Number of files with no model imports.
    no_models: AtomicU64,
    /// Number of files that failed to scan (read or parse errors).
    errors: AtomicU64,
}

impl ScanStats {
    /// Creates a new [`ScanStats`] with all counters at zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanStats;
    ///
    /// let stats = ScanStats::new();
    /// assert_eq!(stats.snapshot().total, 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Increments the total files counter.
    #[inline]
    pub fn increment_total(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the legacy files counter.
    #[inline]
    pub fn increment_legacy(&self) {
        self.legacy.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the migrated files counter.
    #[inline]
    pub fn increment_migrated(&self) {
        self.migrated.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the partial migration counter.
    #[inline]
    pub fn increment_partial(&self) {
        self.partial.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the no-models counter.
    #[inline]
    pub fn increment_no_models(&self) {
        self.no_models.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the error counter.
    #[inline]
    pub fn increment_errors(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a point-in-time snapshot of all statistics.
    ///
    /// The snapshot is consistent in that all values are read at
    /// approximately the same time, but due to relaxed ordering,
    /// the values may not reflect a perfectly consistent state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::ScanStats;
    ///
    /// let stats = ScanStats::new();
    /// stats.increment_total();
    /// stats.increment_legacy();
    ///
    /// let snap = stats.snapshot();
    /// assert_eq!(snap.total, 1);
    /// assert_eq!(snap.legacy, 1);
    /// ```
    #[must_use]
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            total: self.total.load(Ordering::Relaxed),
            legacy: self.legacy.load(Ordering::Relaxed),
            migrated: self.migrated.load(Ordering::Relaxed),
            partial: self.partial.load(Ordering::Relaxed),
            no_models: self.no_models.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }

    /// Resets all counters to zero.
    ///
    /// Useful for re-scanning.
    pub fn reset(&self) {
        self.total.store(0, Ordering::Relaxed);
        self.legacy.store(0, Ordering::Relaxed);
        self.migrated.store(0, Ordering::Relaxed);
        self.partial.store(0, Ordering::Relaxed);
        self.no_models.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
    }
}

/// A point-in-time snapshot of scan statistics.
///
/// This struct contains copied values from [`ScanStats`] and is safe to
/// store, serialize, and send between threads.
///
/// # Examples
///
/// ```
/// use ch_scanner::{ScanStats, StatsSnapshot};
///
/// let stats = ScanStats::new();
/// stats.increment_total();
/// stats.increment_legacy();
///
/// let snapshot: StatsSnapshot = stats.snapshot();
/// println!("Migration progress: {:.1}%", snapshot.progress_percent());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StatsSnapshot {
    /// Total number of files scanned.
    pub total: u64,
    /// Number of files with only legacy imports.
    pub legacy: u64,
    /// Number of files with only migrated imports.
    pub migrated: u64,
    /// Number of files with both legacy and migrated imports.
    pub partial: u64,
    /// Number of files with no model imports.
    pub no_models: u64,
    /// Number of files that failed to scan.
    pub errors: u64,
}

impl StatsSnapshot {
    /// Returns the migration progress as a percentage.
    ///
    /// Calculated as: `migrated / (legacy + migrated + partial) * 100`
    ///
    /// Files with no model imports are excluded from the calculation.
    /// Returns 100.0 if there are no files with model imports.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::StatsSnapshot;
    ///
    /// let snap = StatsSnapshot {
    ///     total: 100,
    ///     legacy: 30,
    ///     migrated: 60,
    ///     partial: 10,
    ///     no_models: 0,
    ///     errors: 0,
    /// };
    ///
    /// assert!((snap.progress_percent() - 60.0).abs() < 0.1);
    /// ```
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Acceptable for statistics display
    pub fn progress_percent(&self) -> f64 {
        let total_with_models = self.legacy + self.migrated + self.partial;
        if total_with_models == 0 {
            return 100.0;
        }

        (self.migrated as f64 / total_with_models as f64) * 100.0
    }

    /// Returns the number of files that need migration.
    ///
    /// This includes both legacy and partial files.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::StatsSnapshot;
    ///
    /// let snap = StatsSnapshot {
    ///     total: 100,
    ///     legacy: 30,
    ///     migrated: 60,
    ///     partial: 10,
    ///     no_models: 0,
    ///     errors: 0,
    /// };
    ///
    /// assert_eq!(snap.needs_migration(), 40);
    /// ```
    #[inline]
    #[must_use]
    pub const fn needs_migration(&self) -> u64 {
        self.legacy + self.partial
    }

    /// Returns the number of files with model imports (excluding `no_models`).
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::StatsSnapshot;
    ///
    /// let snap = StatsSnapshot {
    ///     total: 100,
    ///     legacy: 30,
    ///     migrated: 60,
    ///     partial: 10,
    ///     no_models: 20,
    ///     errors: 0,
    /// };
    ///
    /// assert_eq!(snap.with_models(), 100);
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_models(&self) -> u64 {
        self.legacy + self.migrated + self.partial
    }

    /// Returns the success rate as a percentage.
    ///
    /// Calculated as: `(total - errors) / total * 100`
    /// Returns 100.0 if total is 0.
    ///
    /// # Examples
    ///
    /// ```
    /// use ch_scanner::StatsSnapshot;
    ///
    /// let snap = StatsSnapshot {
    ///     total: 100,
    ///     legacy: 30,
    ///     migrated: 60,
    ///     partial: 5,
    ///     no_models: 0,
    ///     errors: 5,
    /// };
    ///
    /// assert!((snap.success_rate() - 95.0).abs() < 0.1);
    /// ```
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Acceptable for statistics display
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            return 100.0;
        }

        ((self.total - self.errors) as f64 / self.total as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_stats_new() {
        let stats = ScanStats::new();
        let snap = stats.snapshot();
        assert_eq!(snap.total, 0);
        assert_eq!(snap.legacy, 0);
        assert_eq!(snap.migrated, 0);
        assert_eq!(snap.partial, 0);
        assert_eq!(snap.no_models, 0);
        assert_eq!(snap.errors, 0);
    }

    #[test]
    fn test_scan_stats_increment() {
        let stats = ScanStats::new();

        stats.increment_total();
        stats.increment_total();
        stats.increment_legacy();
        stats.increment_migrated();
        stats.increment_partial();
        stats.increment_no_models();
        stats.increment_errors();

        let snap = stats.snapshot();
        assert_eq!(snap.total, 2);
        assert_eq!(snap.legacy, 1);
        assert_eq!(snap.migrated, 1);
        assert_eq!(snap.partial, 1);
        assert_eq!(snap.no_models, 1);
        assert_eq!(snap.errors, 1);
    }

    #[test]
    fn test_scan_stats_reset() {
        let stats = ScanStats::new();
        stats.increment_total();
        stats.increment_legacy();

        stats.reset();

        let snap = stats.snapshot();
        assert_eq!(snap.total, 0);
        assert_eq!(snap.legacy, 0);
    }

    #[test]
    fn test_stats_snapshot_progress_percent() {
        // No files with models -> 100%
        let snap = StatsSnapshot::default();
        assert!((snap.progress_percent() - 100.0).abs() < f64::EPSILON);

        // All migrated -> 100%
        let snap = StatsSnapshot {
            total: 100,
            migrated: 100,
            ..Default::default()
        };
        assert!((snap.progress_percent() - 100.0).abs() < f64::EPSILON);

        // 50% migrated
        let snap = StatsSnapshot {
            total: 100,
            legacy: 50,
            migrated: 50,
            ..Default::default()
        };
        assert!((snap.progress_percent() - 50.0).abs() < f64::EPSILON);

        // Mixed case
        let snap = StatsSnapshot {
            total: 100,
            legacy: 30,
            migrated: 60,
            partial: 10,
            no_models: 0,
            errors: 0,
        };
        assert!((snap.progress_percent() - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_snapshot_needs_migration() {
        let snap = StatsSnapshot {
            total: 100,
            legacy: 30,
            migrated: 50,
            partial: 20,
            no_models: 0,
            errors: 0,
        };
        assert_eq!(snap.needs_migration(), 50);
    }

    #[test]
    fn test_stats_snapshot_with_models() {
        let snap = StatsSnapshot {
            total: 120,
            legacy: 30,
            migrated: 50,
            partial: 20,
            no_models: 20,
            errors: 0,
        };
        assert_eq!(snap.with_models(), 100);
    }

    #[test]
    fn test_stats_snapshot_success_rate() {
        // No files -> 100%
        let snap = StatsSnapshot::default();
        assert!((snap.success_rate() - 100.0).abs() < f64::EPSILON);

        // No errors -> 100%
        let snap = StatsSnapshot {
            total: 100,
            legacy: 100,
            ..Default::default()
        };
        assert!((snap.success_rate() - 100.0).abs() < f64::EPSILON);

        // 5% errors -> 95%
        let snap = StatsSnapshot {
            total: 100,
            legacy: 95,
            errors: 5,
            ..Default::default()
        };
        assert!((snap.success_rate() - 95.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_snapshot_serialization() {
        let snap = StatsSnapshot {
            total: 100,
            legacy: 30,
            migrated: 60,
            partial: 10,
            no_models: 0,
            errors: 0,
        };

        let json = serde_json::to_string(&snap).expect("Serialization failed");
        let parsed: StatsSnapshot = serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(snap, parsed);
    }
}
