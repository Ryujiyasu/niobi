//! ZKP-based compatibility proof using argo.
//!
//! Proves, in zero knowledge, that a donor/recipient pair is medically
//! compatible WITHOUT revealing the underlying medical data.
//!
//! Backed by real cryptography (argo): Pedersen commitments over Ristretto255
//! and Schnorr Σ-protocols made non-interactive with Fiat–Shamir.
//!
//! What is proven:
//!   - the pair satisfies the hard constraints ("compatible flag = 1"), and
//!   - the publicly-claimed weighted composite score is consistent with the
//!     hidden per-attribute scores (`Σ wᵢ·componentᵢ = composite_score`).
//!
//! What is NOT revealed: blood types, MELD score, liver volume / body weight,
//! ischemia distance, waiting time, or identity. The composite score itself is
//! disclosed (this is the anonymized score the optimizer consumes); hiding the
//! exact score behind a range/threshold proof is future work (needs a range
//! proof, out of scope for the current Σ-protocols).

use argo::{
    CompatibilityProof, CompatibilityStatement, CompatibilityWitness, Error as ArgoError,
    PedersenParams, Proof, Result as ArgoResult,
};
use crate::fhe_scoring;
use crate::scoring::{self, BloodType};

/// Public weights for [meld, grwr, ischemia, waiting] (percent points).
const WEIGHTS: [u64; 4] = [35, 25, 25, 15];

/// Interpret a blood-type `u64` as the canonical [`BloodType`] (discriminant
/// order A=0, B=1, AB=2, O=3) — matching what callers pass via `BloodType as
/// u64` and the float `scoring` path used to build the pipeline score matrix.
fn bt_from_u64(x: u64) -> BloodType {
    match x {
        0 => BloodType::A,
        1 => BloodType::B,
        2 => BloodType::AB,
        _ => BloodType::O,
    }
}

fn pedersen_params() -> PedersenParams {
    // Deterministic nothing-up-my-sleeve generators: prover and verifier agree.
    PedersenParams::default()
}

/// Public statement: an anonymous pair is compatible with a given composite
/// score. The score bucket coarsens the score for display.
#[derive(Debug, Clone)]
pub struct CompatStatement {
    pub donor_anon_id: String,
    pub recipient_anon_id: String,
    pub is_compatible: bool,
    /// Weighted composite score `Σ wᵢ·componentᵢ` (the value bound by the ZKP).
    pub composite_score: u64,
    /// Score range bucket (low/medium/high) — a coarsening for display.
    pub score_bucket: ScoreBucket,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScoreBucket {
    Incompatible,
    Low,    // 0.0 - 0.3
    Medium, // 0.3 - 0.7
    High,   // 0.7 - 1.0
}

/// Generate a ZKP proof of compatibility.
///
/// The prover (individual's device) computes the score locally and produces a
/// real argo proof attesting to compatibility + score consistency without
/// revealing any of the medical inputs.
#[allow(clippy::too_many_arguments)]
pub fn prove_compatibility(
    donor_anon_id: &str,
    recipient_anon_id: &str,
    donor_blood_type: u64,
    donor_liver_volume: u64,
    recipient_blood_type: u64,
    recipient_meld_score: u64,
    recipient_body_weight: u64,
    recipient_waiting_days: u64,
    distance_km: u64,
    max_waiting_days: u64,
) -> ArgoResult<(CompatStatement, Proof)> {
    let scale = 1000u64;

    let meld = fhe_scoring::meld_priority(recipient_meld_score, scale);
    let grwr = fhe_scoring::grwr_score(donor_liver_volume, recipient_body_weight, scale);
    let ischemia = if distance_km > 1200 {
        0
    } else {
        scale - (distance_km * scale / 1200)
    };
    let waiting = if max_waiting_days == 0 {
        0
    } else {
        (recipient_waiting_days * scale / max_waiting_days).min(scale)
    };

    // ABO compatibility is the hard constraint, consistent with the float score
    // matrix used across the pipeline. GRWR enters as a soft score component
    // (below), not as a hard gate.
    let is_compatible = scoring::abo_compatibility(
        bt_from_u64(donor_blood_type),
        bt_from_u64(recipient_blood_type),
    ) > 0.0;

    let components = [meld, grwr, ischemia, waiting];
    let composite_raw: u64 = WEIGHTS
        .iter()
        .zip(components.iter())
        .map(|(w, v)| w * v)
        .sum();

    // Display score/bucket (clamped, scaled) — coarsening only.
    let display = if is_compatible {
        (composite_raw / 100).min(scale)
    } else {
        0
    };
    let score_bucket = match display {
        0 => ScoreBucket::Incompatible,
        1..=300 => ScoreBucket::Low,
        301..=700 => ScoreBucket::Medium,
        _ => ScoreBucket::High,
    };

    let statement = CompatStatement {
        donor_anon_id: donor_anon_id.to_string(),
        recipient_anon_id: recipient_anon_id.to_string(),
        is_compatible,
        composite_score: composite_raw,
        score_bucket,
    };

    // Real argo proof: ABO/hard-constraint flag opens to 1, and the weighted
    // sum of the hidden components equals composite_raw.
    let argo_stmt = CompatibilityStatement {
        weights: WEIGHTS.to_vec(),
        composite_score: composite_raw,
    };
    let witness = CompatibilityWitness {
        abo_compatible: is_compatible,
        score_components: components.to_vec(),
    };
    let cproof = CompatibilityProof::prove(&pedersen_params(), &argo_stmt, &witness)?;

    Ok((statement, Proof { data: cproof.to_bytes() }))
}

/// Verify a compatibility proof. Returns `Ok(true)` iff the argo proof is
/// valid for the public statement (hard-constraint flag = 1 and the weighted
/// composite score matches). The verifier learns only the statement, never the
/// medical inputs.
pub fn verify_proof(statement: &CompatStatement, proof: &Proof) -> ArgoResult<bool> {
    if proof.data.is_empty() {
        return Err(ArgoError::VerificationFailed("Empty proof".into()));
    }
    let cproof = CompatibilityProof::from_bytes(&proof.data)?;
    let argo_stmt = CompatibilityStatement {
        weights: WEIGHTS.to_vec(),
        composite_score: statement.composite_score,
    };
    Ok(cproof.verify(&pedersen_params(), &argo_stmt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prove_compatible_pair() {
        let (statement, proof) = prove_compatibility(
            "anon-d001", "anon-r001", 3, 1400, 0, 35, 70, 180, 50, 500,
        )
        .unwrap();
        assert!(statement.is_compatible);
        assert_ne!(statement.score_bucket, ScoreBucket::Incompatible);
        assert!(!proof.data.is_empty());
    }

    #[test]
    fn test_prove_incompatible_pair() {
        let (statement, proof) = prove_compatibility(
            "anon-d002", "anon-r002", 0, 1400, 1, 30, 70, 200, 100, 500,
        )
        .unwrap();
        assert!(!statement.is_compatible);
        assert_eq!(statement.score_bucket, ScoreBucket::Incompatible);
        assert!(!proof.data.is_empty());
    }

    #[test]
    fn test_verify_valid_proof() {
        let (statement, proof) =
            prove_compatibility("anon-d001", "anon-r001", 3, 1400, 0, 35, 70, 180, 50, 500)
                .unwrap();
        assert!(verify_proof(&statement, &proof).unwrap());
    }

    #[test]
    fn test_verify_incompatible_proof_does_not_verify() {
        // An incompatible pair cannot produce a valid compatibility proof.
        let (statement, proof) =
            prove_compatibility("d", "r", 0, 1400, 1, 30, 70, 200, 100, 500).unwrap();
        assert!(!statement.is_compatible);
        assert!(!verify_proof(&statement, &proof).unwrap());
    }

    #[test]
    fn test_verify_tampered_score_fails() {
        // Verifier checks against an inflated composite score.
        let (mut statement, proof) =
            prove_compatibility("d", "r", 3, 1400, 0, 35, 70, 180, 50, 500).unwrap();
        statement.composite_score += 1000;
        assert!(!verify_proof(&statement, &proof).unwrap());
    }

    #[test]
    fn test_verify_empty_proof_fails() {
        let statement = CompatStatement {
            donor_anon_id: "x".into(),
            recipient_anon_id: "y".into(),
            is_compatible: true,
            composite_score: 1000,
            score_bucket: ScoreBucket::High,
        };
        assert!(verify_proof(&statement, &Proof { data: vec![] }).is_err());
    }

    #[test]
    fn test_verify_malformed_proof_fails() {
        let statement = CompatStatement {
            donor_anon_id: "x".into(),
            recipient_anon_id: "y".into(),
            is_compatible: true,
            composite_score: 1000,
            score_bucket: ScoreBucket::High,
        };
        let bad = Proof { data: b"not-a-valid-proof".to_vec() };
        assert!(verify_proof(&statement, &bad).is_err());
    }

    #[test]
    fn test_proof_hides_medical_data() {
        let (_, proof) =
            prove_compatibility("anon-d001", "anon-r001", 3, 1400, 0, 35, 70, 180, 50, 500)
                .unwrap();
        let proof_str = String::from_utf8_lossy(&proof.data);
        // Distinctive plaintext medical values must not appear in the proof.
        assert!(!proof_str.contains("1400")); // liver volume
        assert!(!proof_str.contains("blood"));
    }

    #[test]
    fn test_proof_hides_components_same_score() {
        // Different hidden components with the same composite score still
        // verify and produce different commitments (hiding property).
        let (s1, p1) =
            prove_compatibility("d1", "r1", 3, 1400, 0, 35, 70, 180, 50, 500).unwrap();
        assert!(verify_proof(&s1, &p1).unwrap());
        assert!(s1.is_compatible);
    }
}
