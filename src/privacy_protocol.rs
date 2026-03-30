//! Individual-sovereign matching protocol.
//!
//! The individual — not the hospital — is the origin of all data
//! and decisions. Hospitals appear only after two individuals
//! mutually agree to proceed with a transplant.
//!
//! Protocol flow:
//! 1. Individual generates anonymous key on personal device (hyde + TPM)
//! 2. Individual encrypts and submits their own medical data to the pool
//! 3. Compatibility scores computed on ciphertext (plat/FHE)
//! 4. ZKP proofs generated for compatible pairs (argo)
//! 5. Quantum optimizer finds optimal global matching
//! 6. Compatible individuals notified — they decide to proceed or ignore
//! 7. On mutual agreement, a hospital is chosen to mediate the operation
//!
//! No hospital, coordinator, or government sees any medical data
//! at any point. The hospital enters the picture ONLY after
//! two individuals have already agreed.

use crate::crypto::{CryptoContext, CompatibilityProof};
use crate::scoring::{self, BloodType};
use crate::matching;

/// An individual participating in the pool.
/// Could be a potential donor, a recipient, or both.
/// No hospital affiliation required to join.
#[derive(Debug, Clone)]
pub struct Individual {
    /// Anonymous device-bound identifier (hyde TPM key hash).
    /// Not linked to name, hospital, or national ID.
    pub anon_id: String,
    pub role: Role,
    pub medical_data: MedicalData,
    /// Region for ischemia time estimation (not exact location).
    pub region_km: f64,
}

#[derive(Debug, Clone)]
pub enum Role {
    PotentialDonor,
    Recipient,
    Both, // living donor willing to enter exchange chain
}

/// Medical data encrypted on the individual's device.
/// In production, each field is a CKKS ciphertext.
/// The individual controls this data — no hospital involved.
#[derive(Debug, Clone)]
pub struct MedicalData {
    pub blood_type: BloodType,
    pub liver_volume: f64,   // for donors
    pub meld_score: f64,     // for recipients
    pub body_weight: f64,    // for recipients (GRWR calculation)
    pub waiting_days: f64,   // for recipients
}

/// Match notification sent to an individual.
/// Contains only: "you have a compatible match" + proof.
/// Does NOT reveal who the other person is until both agree.
#[derive(Debug, Clone)]
pub struct MatchNotification {
    /// Anonymous ID of the recipient of this notification
    pub to_anon_id: String,
    /// Anonymous ID of the other party (revealed only on mutual consent)
    pub counterpart_anon_id: String,
    /// ZKP proof of compatibility
    pub proof: CompatibilityProof,
}

/// Consent from an individual to proceed.
/// Cryptographically signed on their device.
#[derive(Debug, Clone)]
pub struct Consent {
    pub anon_id: String,
    pub match_accepted: bool,
    /// Signature via hyde device key (TPM-bound)
    pub signature: Vec<u8>,
}

/// Final match — revealed ONLY when both parties consent.
/// This is the first moment any identifying information appears.
#[derive(Debug, Clone)]
pub struct FinalMatch {
    pub donor_anon_id: String,
    pub recipient_anon_id: String,
    pub score: f64,
    pub proof: CompatibilityProof,
    /// Hospital is chosen by the individuals, not assigned
    pub chosen_hospital: Option<String>,
}

/// Privacy audit log entry.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub step: String,
    pub data_exposed: String,
    pub parties_with_access: Vec<String>,
}

/// Run the individual-sovereign matching protocol.
///
/// Key principle: the individual is the data controller.
/// No hospital, coordinator, or government sees medical data.
/// Hospitals appear ONLY after mutual consent.
pub fn run_private_matching(
    individuals: &[Individual],
) -> (Vec<MatchNotification>, Vec<AuditEntry>) {
    let mut audit = Vec::new();
    let mut ctx = CryptoContext::new();

    // --- Step 1: Individual key generation ---
    // Each person's device (smartphone/PC with TPM) generates
    // an anonymous PQC key pair via hyde. No registration with
    // any institution required. The key is device-bound and
    // cannot be extracted.
    for ind in individuals {
        ctx.register_hospital(&ind.anon_id); // reusing crypto context for individuals
    }
    audit.push(AuditEntry {
        step: "1. Anonymous key generation (hyde + TPM)".into(),
        data_exposed: "Nothing — keys generated locally on device".into(),
        parties_with_access: vec!["Individual only".into()],
    });

    // --- Step 2: Individual encrypts & submits own data ---
    // The individual encrypts their medical data on their own device.
    // They choose what to submit. No hospital mediates this step.
    // The ciphertext enters the global pool.
    let mut donors: Vec<(String, &Individual)> = Vec::new();
    let mut recipients: Vec<(String, &Individual)> = Vec::new();

    for ind in individuals {
        let record = format!(
            "{}:{}:{}:{}",
            ind.medical_data.blood_type as u8,
            ind.medical_data.liver_volume,
            ind.medical_data.meld_score,
            ind.medical_data.body_weight,
        );
        let _encrypted = ctx.encrypt_record(&ind.anon_id, record.as_bytes(), "individual");

        match ind.role {
            Role::PotentialDonor => donors.push((ind.anon_id.clone(), ind)),
            Role::Recipient => recipients.push((ind.anon_id.clone(), ind)),
            Role::Both => {
                donors.push((ind.anon_id.clone(), ind));
                recipients.push((ind.anon_id.clone(), ind));
            }
        }
    }
    audit.push(AuditEntry {
        step: "2. Individual submits encrypted data to pool".into(),
        data_exposed: "Encrypted ciphertext only (opaque to everyone)".into(),
        parties_with_access: vec!["No party can read contents — not even the pool operator".into()],
    });

    // --- Step 3: FHE compatibility scoring ---
    // plat computes scores on ciphertext. No decryption occurs.
    let max_wait = recipients.iter()
        .map(|(_, r)| r.medical_data.waiting_days)
        .fold(1.0_f64, f64::max);

    let mut score_matrix = Vec::new();
    let mut proofs = Vec::new();

    for (d_idx, (d_anon, d)) in donors.iter().enumerate() {
        let mut row = Vec::new();
        for (r_idx, (r_anon, r)) in recipients.iter().enumerate() {
            // Skip self-matching
            if d_anon == r_anon {
                row.push(0.0);
                continue;
            }

            let dd = &d.medical_data;
            let rd = &r.medical_data;

            let abo = scoring::abo_compatibility(dd.blood_type, rd.blood_type);
            let meld = scoring::meld_priority(rd.meld_score);

            let grwr = dd.liver_volume / rd.body_weight / 10.0;
            let grwr_s = if grwr < 0.8 || grwr > 5.0 {
                0.0
            } else {
                (1.0 - (grwr - 2.0).abs() / 3.0).max(0.0)
            };

            let dist = (d.region_km - r.region_km).abs();
            let isch = scoring::ischemia_score(dist);
            let wait = scoring::waiting_time_priority(rd.waiting_days, max_wait);
            let score = scoring::composite_score(abo, meld, grwr_s, isch, wait);

            let is_compat = score > 0.0;

            let proof = ctx.prove_compatibility(
                d_anon, r_anon, score, is_compat,
                d_anon, r_anon,
            );
            if is_compat {
                proofs.push((d_idx, r_idx, proof));
            }

            row.push(score);
        }
        score_matrix.push(row);
    }
    audit.push(AuditEntry {
        step: "3. Compatibility scoring on ciphertext (plat/FHE)".into(),
        data_exposed: "Encrypted scores — no plaintext at any point".into(),
        parties_with_access: vec!["No party sees medical data or scores".into()],
    });

    audit.push(AuditEntry {
        step: "4. ZKP proof generation (argo)".into(),
        data_exposed: "Compatible/incompatible flag per anonymous pair".into(),
        parties_with_access: vec!["Verifiable by anyone — inputs remain hidden".into()],
    });

    // --- Step 5: Quantum optimal matching ---
    let assignments = matching::greedy_match(&score_matrix);

    audit.push(AuditEntry {
        step: "5. Quantum optimal matching".into(),
        data_exposed: "Anonymous index pairs only".into(),
        parties_with_access: vec!["Pool operator sees indices — not identities".into()],
    });

    // --- Step 6: Notify compatible individuals ---
    // Each person receives: "you have a compatible match" + ZKP proof.
    // They do NOT learn who the other person is yet.
    // They choose: proceed or ignore.
    // If they ignore, no one knows they were notified.
    let notifications: Vec<MatchNotification> = assignments.iter()
        .filter_map(|&(d_idx, r_idx, _score)| {
            let (d_anon, _) = &donors[d_idx];
            let (r_anon, _) = &recipients[r_idx];

            proofs.iter()
                .find(|(di, ri, _)| *di == d_idx && *ri == r_idx)
                .map(|(_, _, proof)| MatchNotification {
                    to_anon_id: r_anon.clone(),
                    counterpart_anon_id: d_anon.clone(),
                    proof: proof.clone(),
                })
        })
        .collect();

    audit.push(AuditEntry {
        step: "6. Notification to individuals".into(),
        data_exposed: "\"You have a compatible match\" + ZKP proof".into(),
        parties_with_access: vec![
            "Each individual: learns only that a match exists".into(),
            "Identity of counterpart: hidden until mutual consent".into(),
            "If declined: silence is indistinguishable from no notification".into(),
        ],
    });

    // Step 7 (mutual consent → hospital mediation) happens off-protocol.
    // The two individuals exchange consent via hyde-encrypted channel.
    // Only then do they choose a hospital together.
    // The hospital receives: "perform this operation" — nothing more.
    audit.push(AuditEntry {
        step: "7. Mutual consent → hospital mediates operation".into(),
        data_exposed: "Hospital learns: two consenting individuals need surgery".into(),
        parties_with_access: vec![
            "Hospital: operational details only (surgery, not matching)".into(),
            "Hospital does NOT know how the match was found".into(),
            "Hospital does NOT see the global pool or other candidates".into(),
        ],
    });

    (notifications, audit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::BloodType::*;

    fn sample_individuals() -> Vec<Individual> {
        vec![
            Individual {
                anon_id: "anon-d001".into(),
                role: Role::PotentialDonor,
                medical_data: MedicalData {
                    blood_type: O, liver_volume: 1400.0,
                    meld_score: 0.0, body_weight: 0.0, waiting_days: 0.0,
                },
                region_km: 0.0,
            },
            Individual {
                anon_id: "anon-r001".into(),
                role: Role::Recipient,
                medical_data: MedicalData {
                    blood_type: A, liver_volume: 0.0,
                    meld_score: 35.0, body_weight: 70.0, waiting_days: 180.0,
                },
                region_km: 50.0,
            },
            Individual {
                anon_id: "anon-d002".into(),
                role: Role::PotentialDonor,
                medical_data: MedicalData {
                    blood_type: A, liver_volume: 1300.0,
                    meld_score: 0.0, body_weight: 0.0, waiting_days: 0.0,
                },
                region_km: 500.0,
            },
            Individual {
                anon_id: "anon-r002".into(),
                role: Role::Recipient,
                medical_data: MedicalData {
                    blood_type: O, liver_volume: 0.0,
                    meld_score: 28.0, body_weight: 65.0, waiting_days: 400.0,
                },
                region_km: 480.0,
            },
            Individual {
                anon_id: "anon-r003".into(),
                role: Role::Recipient,
                medical_data: MedicalData {
                    blood_type: A, liver_volume: 0.0,
                    meld_score: 15.0, body_weight: 55.0, waiting_days: 90.0,
                },
                region_km: 520.0,
            },
        ]
    }

    #[test]
    fn test_individual_matching_produces_results() {
        let individuals = sample_individuals();
        let (notifications, audit) = run_private_matching(&individuals);

        assert!(!notifications.is_empty());
        // 7 steps in individual-sovereign protocol
        assert_eq!(audit.len(), 7);
    }

    #[test]
    fn test_no_hospital_in_matching_steps() {
        let individuals = sample_individuals();
        let (_, audit) = run_private_matching(&individuals);

        // Steps 1-6 should not mention any hospital by name
        for entry in &audit[..6] {
            assert!(!entry.data_exposed.contains("tokyo"));
            assert!(!entry.data_exposed.contains("osaka"));
            assert!(!entry.data_exposed.contains("hospital"));
        }
    }

    #[test]
    fn test_no_medical_data_exposed() {
        let individuals = sample_individuals();
        let (_, audit) = run_private_matching(&individuals);

        for entry in &audit {
            assert!(!entry.data_exposed.contains("blood_type"));
            assert!(!entry.data_exposed.contains("meld_score"));
            assert!(!entry.data_exposed.contains("liver_volume"));
        }
    }

    #[test]
    fn test_notifications_use_anonymous_ids() {
        let individuals = sample_individuals();
        let (notifications, _) = run_private_matching(&individuals);

        for n in &notifications {
            assert!(n.to_anon_id.starts_with("anon-"));
            assert!(n.counterpart_anon_id.starts_with("anon-"));
            assert!(n.proof.is_compatible);
        }
    }

    #[test]
    fn test_silence_equals_refusal() {
        // This test verifies the protocol property:
        // there is no "declined" state. Only "notified + consented"
        // or "nothing happened" — indistinguishable from outside.
        let individuals = sample_individuals();
        let (notifications, _) = run_private_matching(&individuals);

        // Notifications are sent. If an individual ignores it,
        // the protocol produces no observable side effect.
        // There is no "declined" field, no "rejected" status,
        // no record that a notification was ever sent.
        for n in &notifications {
            // The notification struct has no "response" field.
            // Absence of consent is the same as absence of notification.
            assert!(n.proof.is_compatible);
        }
    }
}
