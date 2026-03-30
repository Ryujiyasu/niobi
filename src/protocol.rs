//! End-to-end matching protocol (Hyde × PLAT × Argo).
//!
//! This module orchestrates the full privacy-preserving matching flow:
//! 1. Hospitals submit encrypted records (Hyde)
//! 2. Compatibility scores computed on ciphertext (PLAT)
//! 3. Optimal assignment solved (Argo)
//! 4. Only final pairing revealed
//!
//! Current implementation uses plaintext for prototyping.
//! FHE lifting is tracked as a separate milestone.

use crate::scoring::{self, BloodType};
use crate::matching;

/// Donor record (will be encrypted via Hyde in production).
#[derive(Debug, Clone)]
pub struct DonorRecord {
    pub id: String,
    pub hospital: String,
    pub blood_type: BloodType,
    pub bsa: f64,           // body surface area (m²)
    pub location_km: f64,   // distance from coordination center
}

/// Recipient record (will be encrypted via Hyde in production).
#[derive(Debug, Clone)]
pub struct RecipientRecord {
    pub id: String,
    pub hospital: String,
    pub blood_type: BloodType,
    pub meld_score: f64,    // 6-40
    pub bsa: f64,           // body surface area (m²)
    pub location_km: f64,   // distance from coordination center
    pub waiting_days: f64,  // days on waitlist
}

/// Match result (the only information revealed to participants).
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub donor_id: String,
    pub recipient_id: String,
    pub score: f64,
}

/// Run the full matching protocol.
///
/// In the encrypted version:
/// - donor/recipient records arrive as CKKS ciphertexts
/// - score computation uses FHE operations
/// - only the final assignment indices are decrypted
pub fn run_matching(
    donors: &[DonorRecord],
    recipients: &[RecipientRecord],
) -> Vec<MatchResult> {
    let max_waiting = recipients
        .iter()
        .map(|r| r.waiting_days)
        .fold(1.0_f64, f64::max);

    // Build score matrix (PLAT layer — will be FHE in production)
    let scores: Vec<Vec<f64>> = donors
        .iter()
        .map(|d| {
            recipients
                .iter()
                .map(|r| {
                    let abo = scoring::abo_compatibility(d.blood_type, r.blood_type);
                    let meld = scoring::meld_priority(r.meld_score);
                    let size = scoring::size_match(d.bsa, r.bsa);
                    let dist = (d.location_km - r.location_km).abs();
                    let ischemia = scoring::ischemia_score(dist);
                    let waiting = scoring::waiting_time_priority(r.waiting_days, max_waiting);
                    scoring::composite_score(abo, meld, size, ischemia, waiting)
                })
                .collect()
        })
        .collect();

    // Solve assignment (Argo layer)
    let assignments = matching::greedy_match(&scores);

    // Reveal only the pairing
    assignments
        .into_iter()
        .map(|(d, r, score)| MatchResult {
            donor_id: donors[d].id.clone(),
            recipient_id: recipients[r].id.clone(),
            score,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::BloodType::*;

    #[test]
    fn test_protocol_basic() {
        let donors = vec![
            DonorRecord {
                id: "D001".into(),
                hospital: "Tokyo Medical".into(),
                blood_type: O,
                bsa: 1.8,
                location_km: 0.0,
            },
        ];
        let recipients = vec![
            RecipientRecord {
                id: "R001".into(),
                hospital: "Osaka University".into(),
                blood_type: A,
                meld_score: 35.0,
                bsa: 1.7,
                location_km: 500.0,
                waiting_days: 180.0,
            },
            RecipientRecord {
                id: "R002".into(),
                hospital: "Tokyo Medical".into(),
                blood_type: B,
                meld_score: 20.0,
                bsa: 1.9,
                location_km: 10.0,
                waiting_days: 90.0,
            },
        ];

        let results = run_matching(&donors, &recipients);
        assert_eq!(results.len(), 1);
        // O-type donor should match — both recipients are compatible
        // Higher MELD (R001) should win despite distance
        println!("Match: {} -> {} (score: {:.3})", results[0].donor_id, results[0].recipient_id, results[0].score);
    }
}
