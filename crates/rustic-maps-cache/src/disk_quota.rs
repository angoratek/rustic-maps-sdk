//! Disk quota resolution: absolute byte caps and free-disk shares.
//!
//! [`CacheQuota`] resolves a byte budget from two optional constraints —
//! an absolute maximum and a percentage share of currently free disk space —
//! taking the smaller whenever both are set. Free space comes from a
//! [`DiskSpaceProbe`] so tests can drive eviction through a fixed synthetic
//! probe without touching the real filesystem.

use geospatial_types::{CacheOperationFailureReason, SDKError};
use std::path::Path;

/// Source of the currently available disk space under the cache.
pub trait DiskSpaceProbe: Send + Sync {
    /// Free bytes on the filesystem holding `cache_file_path`.
    fn available_disk_bytes(&self, cache_file_path: &Path) -> Result<u64, SDKError>;
}

/// Probe backed by the real filesystem (fs4 statfs family).
pub struct RealDiskSpaceProbe;

impl DiskSpaceProbe for RealDiskSpaceProbe {
    fn available_disk_bytes(&self, cache_file_path: &Path) -> Result<u64, SDKError> {
        fs4::available_space(cache_file_path).map_err(|_| {
            SDKError::CacheOperationFailure(CacheOperationFailureReason::StoreUnavailable)
        })
    }
}

/// Synthetic probe with a constant available byte count, for tests.
pub struct FixedDiskSpaceProbe {
    fixed_available_bytes: u64,
}

impl FixedDiskSpaceProbe {
    /// Pins the reported free space to the given byte count.
    pub fn reporting(fixed_available_bytes: u64) -> Self {
        Self {
            fixed_available_bytes,
        }
    }
}

impl DiskSpaceProbe for FixedDiskSpaceProbe {
    fn available_disk_bytes(&self, _cache_file_path: &Path) -> Result<u64, SDKError> {
        Ok(self.fixed_available_bytes)
    }
}

/// Optional constraints bounding the total cache size on disk.
#[derive(Debug, Clone, Default)]
pub struct CacheQuota {
    /// Hard cap in bytes, independent of filesystem state.
    pub absolute_maximum_bytes: Option<u64>,
    /// Percentage (0..=100) of currently free disk space the cache may use.
    pub free_disk_share_percentage: Option<f64>,
}

impl CacheQuota {
    /// Resolves the effective byte budget: the smallest among the set
    /// constraints, or "unbounded" when none is configured.
    pub fn resolve_budget_bytes(
        &self,
        disk_space_probe: &dyn DiskSpaceProbe,
        cache_file_path: &Path,
    ) -> u64 {
        let mut candidate_budgets: Vec<u64> = Vec::new();
        if let Some(absolute_maximum_bytes) = self.absolute_maximum_bytes {
            candidate_budgets.push(absolute_maximum_bytes);
        }
        if let Some(free_disk_share_percentage) = self.free_disk_share_percentage
            && let Ok(available_disk_bytes) = disk_space_probe.available_disk_bytes(cache_file_path)
        {
            candidate_budgets.push(share_cap(available_disk_bytes, free_disk_share_percentage));
        }
        candidate_budgets.iter().min().copied().unwrap_or(u64::MAX)
    }

    /// Rejects percentage values outside 0..=100.
    pub fn validate(&self) -> Result<(), SDKError> {
        if let Some(free_disk_share_percentage) = self.free_disk_share_percentage
            && !(0.0..=100.0).contains(&free_disk_share_percentage)
        {
            return Err(SDKError::InvalidCoordinateInput(
                "cache free-disk share percentage must be within 0..=100".to_string(),
            ));
        }
        Ok(())
    }
}

fn share_cap(available_disk_bytes: u64, free_disk_share_percentage: f64) -> u64 {
    let share_bytes = (u128::from(available_disk_bytes)
        * (free_disk_share_percentage * 100.0).round() as u128)
        / 10_000;
    share_bytes.min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::{CacheQuota, DiskSpaceProbe, FixedDiskSpaceProbe, RealDiskSpaceProbe};
    use std::path::Path;

    #[test]
    fn absolute_cap_wins_over_small_share() {
        let quota = CacheQuota {
            absolute_maximum_bytes: Some(1_000_000),
            free_disk_share_percentage: Some(50.0),
        };
        let probe = FixedDiskSpaceProbe::reporting(10_000_000);
        assert_eq!(
            quota.resolve_budget_bytes(&probe, Path::new("/tmp")),
            1_000_000
        );
    }

    #[test]
    fn share_cap_wins_over_small_absolute() {
        let quota = CacheQuota {
            absolute_maximum_bytes: Some(1_000),
            free_disk_share_percentage: Some(50.0),
        };
        let probe = FixedDiskSpaceProbe::reporting(10_000_000);
        assert_eq!(quota.resolve_budget_bytes(&probe, Path::new("/tmp")), 1_000);
    }

    #[test]
    fn share_resolves_from_reported_free_space() {
        let quota = CacheQuota {
            absolute_maximum_bytes: None,
            free_disk_share_percentage: Some(25.0),
        };
        let probe = FixedDiskSpaceProbe::reporting(8_000_000);
        assert_eq!(
            quota.resolve_budget_bytes(&probe, Path::new("/tmp")),
            2_000_000
        );
    }

    #[test]
    fn unbounded_quota_has_no_budget() {
        let quota = CacheQuota::default();
        let probe = FixedDiskSpaceProbe::reporting(64);
        assert_eq!(
            quota.resolve_budget_bytes(&probe, Path::new("/tmp")),
            u64::MAX
        );
    }

    #[test]
    fn rejects_out_of_range_percentages() {
        let quota = CacheQuota {
            free_disk_share_percentage: Some(150.0),
            ..CacheQuota::default()
        };
        assert!(quota.validate().is_err());
        assert!(CacheQuota::default().validate().is_ok());
    }

    #[test]
    fn real_probe_reads_a_real_filesystem() {
        let probe = RealDiskSpaceProbe;
        // Any real readable file path works; only a positive value matters.
        let real_path = std::env::temp_dir().join("rustic-maps-cache-quota-probe");
        std::fs::write(&real_path, b"probe").unwrap();
        let available = probe.available_disk_bytes(&real_path).unwrap();
        assert!(available > 0);
        std::fs::remove_file(&real_path).unwrap();
    }
}
