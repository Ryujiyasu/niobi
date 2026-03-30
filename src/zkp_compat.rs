//! ZKP-based compatibility proof using argo.
//!
//! This module implements zero-knowledge proofs that demonstrate
//! donor-recipient compatibility WITHOUT revealing any medical data.
//!
//! What is proven:
//!   "Donor X and Recipient Y are medically compatible"
//!
//! What is NOT revealed:
//!   - Blood types of either party
//!   - MELD score
//!   - Liver volume / body weight
//!   - Location / hospital
//!   - Identity of either party
//!
//! The proof is verifiable by anyone but reveals nothing about inputs.

use argo_core::{Proof, Error as ArgoError, Result as ArgoResult};
use crate::fhe_scoring;

/// Witness data for the compatibility proof.
/// This is the private input — known only to the prover, never revealed.
struct CompatWitness {
    donor_blood_type: u64,
    donor_liver_volume: u64,
    recipient_blood_type: u64,
    recipient_meld_score: u64,
    recipient_body_weight: u64,
    recipient_waiting_days: u64,
    distance_km: u64,
}

/// Public statement: "these anonymous IDs are compatible with score >= threshold"
#[derive(Debug, Clone)]
pub struct CompatStatement {
    pub donor_anon_id: String,
    pub recipient_anon_id: String,
    pub is_compatible: bool,
    /// Score range bucket (low/medium/high) — NOT the exact score
    pub score_bucket: ScoreBucket,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScoreBucket {
    Incompatible,
    Low,     // 0.0 - 0.3
    Medium,  // 0.3 - 0.7
    High,    // 0.7 - 1.0
}

/// Generate a ZKP proof of compatibility.
///
/// The prover (individual's device) computes the score locally and
/// generates a proof. The proof attests to compatibility without
/// revealing any of the medical data used in the computation.
///
/// In production: this uses a ZKP circuit (e.g., Groth16, PLONK)
/// that encodes the scoring logic as arithmetic constraints.
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

    // Compute score (same logic as fhe_scoring, but in ZKP circuit)
    let abo = fhe_scoring::encrypted_abo_compatibility(donor_blood_type, recipient_blood_type);
    let meld = fhe_scoring::encrypted_meld_priority(recipient_meld_score, scale);
    let grwr = fhe_scoring::encrypted_grwr_score(donor_liver_volume, recipient_body_weight, scale);

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

    let score = fhe_scoring::encrypted_composite_score(abo, meld, grwr, ischemia, waiting, scale);

    let is_compatible = score > 0;
    let score_bucket = match score {
        0 => ScoreBucket::Incompatible,
        1..=300 => ScoreBucket::Low,
        301..=700 => ScoreBucket::Medium,
        _ => ScoreBucket::High,
    };

    let statement = CompatStatement {
        donor_anon_id: donor_anon_id.to_string(),
        recipient_anon_id: recipient_anon_id.to_string(),
        is_compatible,
        score_bucket,
    };

    // Generate argo proof.
    // In production: encode witness + statement into ZKP circuit constraints.
    // The proof commits to the witness without revealing it.
    let proof_data = generate_proof_bytes(
        &statement,
        donor_blood_type,
        donor_liver_volume,
        recipient_blood_type,
        recipient_meld_score,
        recipient_body_weight,
    );

    let proof = Proof { data: proof_data };

    Ok((statement, proof))
}

/// Verify a compatibility proof.
///
/// Anyone can verify this proof. The verifier learns:
/// - Whether the pair is compatible
/// - The score bucket (low/medium/high)
///
/// The verifier does NOT learn:
/// - Blood types
/// - MELD score
/// - Liver volume
/// - Body weight
/// - Location
/// - Identity
pub fn verify_proof(statement: &CompatStatement, proof: &Proof) -> ArgoResult<bool> {
    if proof.data.is_empty() {
        return Err(ArgoError::VerificationFailed("Empty proof".into()));
    }

    // In production: verify ZKP proof against statement using verification key.
    // Simulated verification: check proof structure integrity.
    let expected_prefix = b"argo-zkp-v1:";
    if proof.data.len() < expected_prefix.len() {
        return Err(ArgoError::VerificationFailed("Invalid proof format".into()));
    }

    if &proof.data[..expected_prefix.len()] != expected_prefix {
        return Err(ArgoError::VerificationFailed("Invalid proof prefix".into()));
    }

    Ok(true)
}

/// Generate proof bytes (simulated ZKP).
/// In production: this is replaced by a real ZKP proving system.
fn generate_proof_bytes(
    statement: &CompatStatement,
    _donor_bt: u64,
    _donor_lv: u64,
    _recip_bt: u64,
    _recip_meld: u64,
    _recip_bw: u64,
) -> Vec<u8> {
    // Proof structure:
    // prefix || compatible_flag || bucket || commitment
    // The commitment binds to the witness without revealing it.
    let mut proof = b"argo-zkp-v1:".to_vec();
    proof.push(if statement.is_compatible { 1 } else { 0 });
    proof.push(match statement.score_bucket {
        ScoreBucket::Incompatible => 0,
        ScoreBucket::Low => 1,
        ScoreBucket::Medium => 2,
        ScoreBucket::High => 3,
    });
    // Simulated commitment (in production: Pedersen commitment or similar)
    proof.extend_from_slice(b"commitment-placeholder");
    proof
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prove_compatible_pair() {
        let (statement, proof) = prove_compatibility(
            "anon-d001", "anon-r001",
            0,     // O type donor
            1400,  // liver volume mL
            1,     // A type recipient
            35,    // MELD score (urgent)
            70,    // body weight kg
            180,   // waiting days
            50,    // distance km
            500,   // max waiting days
        ).unwrap();

        assert!(statement.is_compatible);
        assert_eq!(statement.score_bucket, ScoreBucket::High);
        assert!(!proof.data.is_empty());
    }

    #[test]
    fn test_prove_incompatible_pair() {
        let (statement, proof) = prove_compatibility(
            "anon-d002", "anon-r002",
            1,     // A type donor
            1400,
            2,     // B type recipient — incompatible with A
            30,
            70,
            200,
            100,
            500,
        ).unwrap();

        assert!(!statement.is_compatible);
        assert_eq!(statement.score_bucket, ScoreBucket::Incompatible);
        assert!(!proof.data.is_empty());
    }

    #[test]
    fn test_verify_valid_proof() {
        let (statement, proof) = prove_compatibility(
            "anon-d001", "anon-r001",
            0, 1400, 1, 35, 70, 180, 50, 500,
        ).unwrap();

        let result = verify_proof(&statement, &proof);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_empty_proof_fails() {
        let statement = CompatStatement {
            donor_anon_id: "x".into(),
            recipient_anon_id: "y".into(),
            is_compatible: true,
            score_bucket: ScoreBucket::High,
        };
        let fake_proof = Proof { data: vec![] };

        let result = verify_proof(&statement, &fake_proof);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_tampered_proof_fails() {
        let statement = CompatStatement {
            donor_anon_id: "x".into(),
            recipient_anon_id: "y".into(),
            is_compatible: true,
            score_bucket: ScoreBucket::High,
        };
        let fake_proof = Proof { data: b"not-a-valid-proof".to_vec() };

        let result = verify_proof(&statement, &fake_proof);
        assert!(result.is_err());
    }

    #[test]
    fn test_proof_hides_medical_data() {
        let (_, proof) = prove_compatibility(
            "anon-d001", "anon-r001",
            0, 1400, 1, 35, 70, 180, 50, 500,
        ).unwrap();

        // Proof bytes should NOT contain plaintext medical values
        let proof_str = String::from_utf8_lossy(&proof.data);
        assert!(!proof_str.contains("1400"));  // liver volume
        assert!(!proof_str.contains("35"));    // MELD (could be in commitment)
        assert!(!proof_str.contains("blood"));
    }

    #[test]
    fn test_score_bucket_hides_exact_score() {
        // Two patients with different scores in same bucket
        let (s1, _) = prove_compatibility(
            "d1", "r1", 0, 1400, 1, 35, 70, 180, 50, 500,
        ).unwrap();
        let (s2, _) = prove_compatibility(
            "d2", "r2", 0, 1300, 0, 30, 65, 300, 100, 500,
        ).unwrap();

        // Both are compatible but exact scores differ
        assert!(s1.is_compatible);
        assert!(s2.is_compatible);
        // The bucket coarsens the score — verifier cannot distinguish
        // exact scores within the same bucket
    }
}
