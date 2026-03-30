//! Compatibility scoring between donors and recipients.
//!
//! In the full protocol, these computations happen on CKKS ciphertext
//! via PLAT. This module defines the scoring logic that will be
//! lifted to FHE operations.

/// Blood type compatibility (ABO system).
/// Returns 1.0 if compatible, 0.0 if not.
pub fn abo_compatibility(donor: BloodType, recipient: BloodType) -> f64 {
    use BloodType::*;
    match (donor, recipient) {
        (O, _) => 1.0,          // O is universal donor
        (A, A) | (A, AB) => 1.0,
        (B, B) | (B, AB) => 1.0,
        (AB, AB) => 1.0,
        _ => 0.0,
    }
}

/// MELD score: Model for End-Stage Liver Disease (6-40).
/// Higher score = more urgent = higher priority.
pub fn meld_priority(meld_score: f64) -> f64 {
    // Normalize to [0, 1] range for scoring
    ((meld_score - 6.0) / 34.0).clamp(0.0, 1.0)
}

/// Size match score based on donor/recipient body surface area ratio.
/// Ideal ratio is close to 1.0; deviations reduce score.
pub fn size_match(donor_bsa: f64, recipient_bsa: f64) -> f64 {
    let ratio = donor_bsa / recipient_bsa;
    // Gaussian-like penalty for size mismatch
    let deviation = (ratio - 1.0).abs();
    (-2.0 * deviation * deviation).exp()
}

/// Cold ischemia time constraint based on distance (hours).
/// Liver must be transplanted within ~12 hours.
pub fn ischemia_score(distance_km: f64) -> f64 {
    let estimated_hours = distance_km / 100.0; // rough transport estimate
    if estimated_hours > 12.0 {
        0.0
    } else {
        1.0 - (estimated_hours / 12.0)
    }
}

/// Waiting time priority (days on waitlist).
pub fn waiting_time_priority(days: f64, max_days: f64) -> f64 {
    (days / max_days).clamp(0.0, 1.0)
}

/// Composite compatibility score.
/// Weights reflect clinical priority: urgency > compatibility > logistics.
pub fn composite_score(
    abo_compat: f64,
    meld: f64,
    size: f64,
    ischemia: f64,
    waiting: f64,
) -> f64 {
    const W_ABO: f64 = 0.30;      // hard constraint (effectively binary gate)
    const W_MELD: f64 = 0.30;     // clinical urgency
    const W_SIZE: f64 = 0.15;     // anatomical fit
    const W_ISCHEMIA: f64 = 0.15; // logistic feasibility
    const W_WAITING: f64 = 0.10;  // fairness / equity

    abo_compat * (W_ABO
        + W_MELD * meld
        + W_SIZE * size
        + W_ISCHEMIA * ischemia
        + W_WAITING * waiting)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BloodType {
    A,
    B,
    AB,
    O,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abo_universal_donor() {
        assert_eq!(abo_compatibility(BloodType::O, BloodType::A), 1.0);
        assert_eq!(abo_compatibility(BloodType::O, BloodType::B), 1.0);
        assert_eq!(abo_compatibility(BloodType::O, BloodType::AB), 1.0);
        assert_eq!(abo_compatibility(BloodType::O, BloodType::O), 1.0);
    }

    #[test]
    fn test_abo_incompatible() {
        assert_eq!(abo_compatibility(BloodType::A, BloodType::B), 0.0);
        assert_eq!(abo_compatibility(BloodType::B, BloodType::A), 0.0);
        assert_eq!(abo_compatibility(BloodType::AB, BloodType::A), 0.0);
    }

    #[test]
    fn test_meld_priority_range() {
        assert_eq!(meld_priority(6.0), 0.0);
        assert_eq!(meld_priority(40.0), 1.0);
        assert!((meld_priority(23.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_size_match_perfect() {
        assert!((size_match(1.8, 1.8) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_ischemia_too_far() {
        assert_eq!(ischemia_score(1500.0), 0.0);
    }

    #[test]
    fn test_composite_incompatible_blood() {
        // ABO incompatibility should zero out the entire score
        let score = composite_score(0.0, 1.0, 1.0, 1.0, 1.0);
        assert_eq!(score, 0.0);
    }
}
