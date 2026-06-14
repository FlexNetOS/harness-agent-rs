//! PORT of `packages/workflows/src/schemas/retry.ts`.
//!
//! UNIT WF-04: `StepRetryConfig` — retry configuration for workflow steps.
//! Source: retry.ts (read directly — ledger inferred from dag-executor.ts:263-279 but
//! the actual file was read; shapes confirmed against source).

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// Serialize an `Option<f64>` the way JS `JSON.stringify` does: an integral value
/// (e.g. `2000.0`) is emitted as an integer (`2000`), a fractional value (`1500.5`)
/// as a float. JS numbers are all f64, and `JSON.stringify(2000.0) === "2000"`, so a
/// faithful round-trip of `delay_ms` must drop the trailing `.0`. Without this, serde_json
/// would emit `2000.0`, diverging from the source's wire shape. retry.ts:14-18.
fn serialize_js_number<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        None => s.serialize_none(),
        // Integral and within the i64 exact-integer range → emit as integer, like JS.
        Some(n) if n.fract() == 0.0 && n.is_finite() && n.abs() < 9_007_199_254_740_992.0 => {
            s.serialize_i64(*n as i64)
        }
        Some(n) => s.serialize_f64(*n),
    }
}

/// Which error types trigger a retry. retry.ts:20.
///
/// `z.enum(['transient', 'all'])` → Rust enum with exact wire names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// Retry only on transient errors (network, timeout, rate limit). retry.ts:20.
    Transient,
    /// Retry on any error. retry.ts:20.
    All,
}

/// Step retry configuration. retry.ts:6-21.
///
/// Zod constraints:
///   - `max_attempts`: `z.number().int().min(1).max(5)` — 1..=5. retry.ts:8-12.
///   - `delay_ms`:     `z.number().min(1000).max(60000).optional()` — 1000..=60000. retry.ts:14-18.
///   - `on_error`:     `z.enum(['transient', 'all']).optional()`. retry.ts:20.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRetryConfig {
    /// Maximum retry attempts (not including the initial attempt). Range: 1..=5. retry.ts:8-12.
    pub max_attempts: u8,

    /// Initial delay in ms; doubled on each attempt. Range: 1000..=60000. retry.ts:14-18.
    ///
    /// `f64`, NOT an integer: source is `z.number().min(1000).max(60000)` with **no `.int()`**
    /// (retry.ts:15), so fractional milliseconds like `1500.5` are source-valid and must be
    /// accepted here too (a `u64` would reject them — a behavioral downgrade).
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_js_number")]
    pub delay_ms: Option<f64>,

    /// Which error types trigger a retry. Default: `transient`. retry.ts:20.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_error: Option<OnError>,
}

/// Validation errors for `StepRetryConfig`. retry.ts:8-20.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum StepRetryValidationError {
    /// `max_attempts` was 0 or > 5. retry.ts:8-12.
    #[error("'retry.max_attempts' must be between 1 and 5")]
    MaxAttemptsOutOfRange,

    /// `delay_ms` was < 1000 or > 60000. retry.ts:14-18.
    #[error("'retry.delay_ms' must be a number between 1000 and 60000")]
    DelayMsOutOfRange,
}

impl StepRetryConfig {
    /// Validate all constraints that zod enforces at parse time.
    ///
    /// Returns all errors found (mirrors zod's collect-all-issues behavior).
    pub fn validate(&self) -> Vec<StepRetryValidationError> {
        let mut errors = Vec::new();

        // z.number().int().min(1).max(5). retry.ts:8-12.
        if self.max_attempts < 1 || self.max_attempts > 5 {
            errors.push(StepRetryValidationError::MaxAttemptsOutOfRange);
        }

        // z.number().min(1000).max(60000).optional() — no `.int()`, so f64. retry.ts:14-18.
        if let Some(delay) = self.delay_ms {
            if !(1000.0..=60_000.0).contains(&delay) {
                errors.push(StepRetryValidationError::DelayMsOutOfRange);
            }
        }

        errors
    }

    /// Parse from a JSON value and immediately validate all constraints.
    pub fn parse(value: serde_json::Value) -> Result<Self, Vec<StepRetryValidationError>> {
        let config: Self = serde_json::from_value(value)
            .map_err(|_| vec![StepRetryValidationError::MaxAttemptsOutOfRange])?;

        let errors = config.validate();
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Accept cases ─────────────────────────────────────────────────────────

    #[test]
    fn valid_minimal_retry_passes() {
        let r = StepRetryConfig {
            max_attempts: 2,
            delay_ms: None,
            on_error: None,
        };
        assert!(r.validate().is_empty());
    }

    #[test]
    fn max_attempts_boundary_1_passes() {
        let r = StepRetryConfig { max_attempts: 1, delay_ms: None, on_error: None };
        assert!(r.validate().is_empty());
    }

    #[test]
    fn max_attempts_boundary_5_passes() {
        let r = StepRetryConfig { max_attempts: 5, delay_ms: None, on_error: None };
        assert!(r.validate().is_empty());
    }

    #[test]
    fn delay_ms_boundary_1000_passes() {
        let r = StepRetryConfig { max_attempts: 1, delay_ms: Some(1000.0), on_error: None };
        assert!(r.validate().is_empty());
    }

    #[test]
    fn delay_ms_boundary_60000_passes() {
        let r = StepRetryConfig { max_attempts: 1, delay_ms: Some(60_000.0), on_error: None };
        assert!(r.validate().is_empty());
    }

    #[test]
    fn delay_ms_fractional_passes() {
        // Source `z.number()` (retry.ts:15) has no `.int()`, so fractional ms is valid.
        let r = StepRetryConfig { max_attempts: 1, delay_ms: Some(1500.5), on_error: None };
        assert!(r.validate().is_empty());
        // Deserialize path: a fractional ms must parse (the WF-04 parity defect that was fixed).
        let parsed = StepRetryConfig::parse(json!({ "max_attempts": 1, "delay_ms": 1500.5 })).unwrap();
        assert_eq!(parsed.delay_ms, Some(1500.5));
        // Wire shape: a fractional value keeps its decimal.
        assert_eq!(serde_json::to_value(&parsed).unwrap()["delay_ms"], 1500.5);
    }

    #[test]
    fn on_error_all_passes() {
        let r = StepRetryConfig {
            max_attempts: 3,
            delay_ms: Some(5000.0),
            on_error: Some(OnError::All),
        };
        assert!(r.validate().is_empty());
    }

    #[test]
    fn round_trip_full_config() {
        let json = json!({
            "max_attempts": 3,
            "delay_ms": 2000,
            "on_error": "transient"
        });
        let r: StepRetryConfig = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(r.max_attempts, 3);
        assert_eq!(r.delay_ms, Some(2000.0));
        assert_eq!(r.on_error, Some(OnError::Transient));
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["max_attempts"], 3);
        // Integral delay re-serializes as an integer (JS `JSON.stringify` parity), not `2000.0`.
        assert_eq!(back["delay_ms"], 2000);
        assert_eq!(back["on_error"], "transient");
    }

    // ── Reject cases ─────────────────────────────────────────────────────────

    #[test]
    fn max_attempts_0_is_rejected() {
        let r = StepRetryConfig { max_attempts: 0, delay_ms: None, on_error: None };
        let errors = r.validate();
        assert!(
            errors.contains(&StepRetryValidationError::MaxAttemptsOutOfRange),
            "got: {errors:?}"
        );
    }

    #[test]
    fn max_attempts_6_is_rejected() {
        let r = StepRetryConfig { max_attempts: 6, delay_ms: None, on_error: None };
        let errors = r.validate();
        assert!(errors.contains(&StepRetryValidationError::MaxAttemptsOutOfRange));
    }

    #[test]
    fn delay_ms_999_is_rejected() {
        let r = StepRetryConfig { max_attempts: 1, delay_ms: Some(999.0), on_error: None };
        let errors = r.validate();
        assert!(errors.contains(&StepRetryValidationError::DelayMsOutOfRange));
    }

    #[test]
    fn delay_ms_60001_is_rejected() {
        let r = StepRetryConfig { max_attempts: 1, delay_ms: Some(60_001.0), on_error: None };
        let errors = r.validate();
        assert!(errors.contains(&StepRetryValidationError::DelayMsOutOfRange));
    }

    // ── Error message exact match ─────────────────────────────────────────────

    #[test]
    fn error_messages_match_zod_exact() {
        assert_eq!(
            StepRetryValidationError::MaxAttemptsOutOfRange.to_string(),
            "'retry.max_attempts' must be between 1 and 5"
        );
        assert_eq!(
            StepRetryValidationError::DelayMsOutOfRange.to_string(),
            "'retry.delay_ms' must be a number between 1000 and 60000"
        );
    }
}
