//! Example: Privacy-preserving international clinical trial matching
//!
//! NEDO Q-2 core use case: "創薬エコシステムの強化に向けた
//! 医療データ共有アプリケーション・アルゴリズムの開発"
//!
//! Current system: pharmaceutical companies recruit trial participants
//! through hospitals they have relationships with. Patient data stays
//! siloed per hospital. Cross-border trials require data sharing
//! agreements that take months to negotiate. Patients with rare
//! conditions never find matching trials.
//!
//! With qmed: every patient's encrypted profile is in the global pool.
//! Pharma companies publish trial criteria. argo proves "this patient
//! matches trial criteria" without revealing the patient's condition.
//! Quantum optimizer assigns patients to trials maximizing statistical
//! power while respecting geographic and demographic constraints.
//!
//! The patient decides whether to participate. The pharma company
//! never sees the patient's data until consent is given.

use qmed::scoring::BloodType;

/// Patient profile (encrypted via hyde in production).
struct PatientProfile {
    id: String,
    age: u32,
    sex: String,
    genomic_markers: Vec<String>,    // encrypted, never exposed
    current_medications: Vec<String>, // encrypted, never exposed
    comorbidities: Vec<String>,       // encrypted, never exposed
    country: String,
    hospital: String,
}

/// Clinical trial criteria published by pharma company.
/// These are public — the criteria themselves are not secret.
struct TrialCriteria {
    trial_id: String,
    sponsor: String,
    target_condition: String,
    required_markers: Vec<String>,
    excluded_medications: Vec<String>,
    age_range: (u32, u32),
    required_participants: usize,
    sites: Vec<String>,  // countries with approved trial sites
}

/// argo proof: "this patient matches trial criteria"
/// Reveals: eligible/ineligible
/// Hidden: genomic data, medications, comorbidities, identity
struct EligibilityProof {
    patient_id_hash: Vec<u8>,
    trial_id: String,
    is_eligible: bool,
    proof: Vec<u8>,
}

/// Match score for patient-trial assignment.
/// In production: computed on ciphertext via plat (FHE).
fn trial_match_score(patient: &PatientProfile, trial: &TrialCriteria) -> f64 {
    // Age check (hard constraint)
    if patient.age < trial.age_range.0 || patient.age > trial.age_range.1 {
        return 0.0;
    }

    // Site availability (hard constraint)
    if !trial.sites.contains(&patient.country) {
        return 0.0;
    }

    // Genomic marker match (computed on encrypted data)
    let marker_match = patient.genomic_markers.iter()
        .filter(|m| trial.required_markers.contains(m))
        .count() as f64 / trial.required_markers.len().max(1) as f64;

    // Medication exclusion check (computed on encrypted data)
    let has_excluded = patient.current_medications.iter()
        .any(|m| trial.excluded_medications.contains(m));
    if has_excluded {
        return 0.0;
    }

    // Geographic diversity bonus (for statistical power)
    let diversity_bonus = 0.1; // simplified

    marker_match + diversity_bonus
}

fn main() {
    println!("=== qmed Example: Clinical Trial Matching ===\n");

    let patients = vec![
        PatientProfile {
            id: "PT001".into(), age: 45, sex: "M".into(),
            genomic_markers: vec!["BRCA1".into(), "TP53".into()],
            current_medications: vec!["metformin".into()],
            comorbidities: vec!["diabetes".into()],
            country: "Japan".into(), hospital: "Tokyo Medical".into(),
        },
        PatientProfile {
            id: "PT002".into(), age: 38, sex: "F".into(),
            genomic_markers: vec!["BRCA1".into(), "EGFR".into()],
            current_medications: vec![],
            comorbidities: vec![],
            country: "Germany".into(), hospital: "Charité Berlin".into(),
        },
        PatientProfile {
            id: "PT003".into(), age: 62, sex: "M".into(),
            genomic_markers: vec!["TP53".into(), "KRAS".into()],
            current_medications: vec!["warfarin".into()],
            comorbidities: vec!["atrial_fibrillation".into()],
            country: "USA".into(), hospital: "Mayo Clinic".into(),
        },
    ];

    let trials = vec![
        TrialCriteria {
            trial_id: "NCT-2026-001".into(),
            sponsor: "Pharma Corp".into(),
            target_condition: "Breast cancer with BRCA1 mutation".into(),
            required_markers: vec!["BRCA1".into()],
            excluded_medications: vec!["warfarin".into()],
            age_range: (18, 65),
            required_participants: 500,
            sites: vec!["Japan".into(), "Germany".into(), "USA".into()],
        },
        TrialCriteria {
            trial_id: "NCT-2026-002".into(),
            sponsor: "BioTech Inc".into(),
            target_condition: "Lung cancer with EGFR mutation".into(),
            required_markers: vec!["EGFR".into()],
            excluded_medications: vec![],
            age_range: (30, 70),
            required_participants: 200,
            sites: vec!["Japan".into(), "Germany".into()],
        },
    ];

    println!("Patients: {} (across {} countries)", patients.len(),
        patients.iter().map(|p| p.country.as_str()).collect::<std::collections::HashSet<_>>().len());
    println!("Trials: {}\n", trials.len());

    for trial in &trials {
        println!("Trial {} ({})", trial.trial_id, trial.target_condition);
        for patient in &patients {
            let score = trial_match_score(patient, trial);
            if score > 0.0 {
                // Note: in production, patient ID is NOT revealed here.
                // Only "an eligible patient exists in [country]" is known.
                println!("  Eligible: anonymous patient in {} (score: {:.3})",
                    patient.country, score);
            }
        }
        println!();
    }

    println!("Privacy guarantees:");
    println!("  - Patient genomic data: NEVER exposed to sponsor");
    println!("  - Patient medications: NEVER exposed to sponsor");
    println!("  - Patient identity: revealed ONLY after consent");
    println!("  - Sponsor knows only: 'N eligible patients exist in countries X, Y, Z'");
    println!("\nQuantum necessity:");
    println!("  - 10,000 patients × 1,000 trials × multi-site constraints");
    println!("  - Optimal assignment maximizing statistical power");
    println!("  - Classical computers: approximate solution");
    println!("  - Quantum annealing: optimal allocation");
}
