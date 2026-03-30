//! Example: Privacy-preserving rare disease patient network
//!
//! Rare diseases affect <200,000 people per condition (US definition).
//! Some conditions have only dozens of known cases worldwide.
//! Each patient's data is in a different hospital, different country,
//! different language, different privacy regime.
//!
//! Researchers can't find enough patients for studies.
//! Patients can't find others with the same condition.
//! Treatments can't be developed without sufficient data.
//!
//! With qmed: every rare disease patient's encrypted profile enters
//! the global pool. Researchers publish study criteria. argo proves
//! "N patients matching criteria exist in countries X, Y, Z" without
//! revealing who they are. Patients are notified and choose to
//! participate. Their data is never exposed until consent.
//!
//! For conditions with 50 patients worldwide, finding even 5 more
//! through the global pool could enable a viable study.

/// Rare disease patient profile (encrypted via hyde).
struct RarePatient {
    id: String,
    condition_code: String,       // ICD-11 code, encrypted
    genetic_variant: String,      // encrypted
    age_at_onset: u32,
    current_treatments: Vec<String>, // encrypted
    biomarker_values: Vec<f64>,      // encrypted
    country: String,
    language: String,
}

/// Research study criteria.
struct StudyCriteria {
    study_id: String,
    condition_code: String,
    target_variants: Vec<String>,
    age_range: (u32, u32),
    min_participants: usize,
    excluded_treatments: Vec<String>,
}

/// argo proof: "N eligible patients exist"
/// Reveals: count per country
/// Hidden: patient identity, genetic data, treatments
struct CohortProof {
    study_id: String,
    eligible_count: usize,
    countries: Vec<String>,
    proof: Vec<u8>,
}

fn check_eligibility(patient: &RarePatient, criteria: &StudyCriteria) -> bool {
    if patient.condition_code != criteria.condition_code {
        return false;
    }
    if patient.age_at_onset < criteria.age_range.0 || patient.age_at_onset > criteria.age_range.1 {
        return false;
    }
    if !criteria.target_variants.is_empty()
        && !criteria.target_variants.contains(&patient.genetic_variant)
    {
        return false;
    }
    if patient.current_treatments.iter()
        .any(|t| criteria.excluded_treatments.contains(t))
    {
        return false;
    }
    true
}

fn main() {
    println!("=== qmed Example: Rare Disease Patient Network ===\n");

    // Scenario: Niemann-Pick disease type C (NPC)
    // Estimated worldwide prevalence: ~1 in 120,000
    let patients = vec![
        RarePatient {
            id: "RP001".into(), condition_code: "5C51.0".into(),
            genetic_variant: "NPC1-I1061T".into(), age_at_onset: 4,
            current_treatments: vec!["miglustat".into()],
            biomarker_values: vec![120.0, 45.0],
            country: "Japan".into(), language: "ja".into(),
        },
        RarePatient {
            id: "RP002".into(), condition_code: "5C51.0".into(),
            genetic_variant: "NPC1-I1061T".into(), age_at_onset: 7,
            current_treatments: vec![],
            biomarker_values: vec![180.0, 62.0],
            country: "France".into(), language: "fr".into(),
        },
        RarePatient {
            id: "RP003".into(), condition_code: "5C51.0".into(),
            genetic_variant: "NPC2-E20X".into(), age_at_onset: 2,
            current_treatments: vec!["arimoclomol".into()],
            biomarker_values: vec![95.0, 38.0],
            country: "USA".into(), language: "en".into(),
        },
        RarePatient {
            id: "RP004".into(), condition_code: "5C51.0".into(),
            genetic_variant: "NPC1-I1061T".into(), age_at_onset: 12,
            current_treatments: vec!["miglustat".into()],
            biomarker_values: vec![150.0, 51.0],
            country: "Germany".into(), language: "de".into(),
        },
        RarePatient {
            id: "RP005".into(), condition_code: "8A40".into(), // different condition
            genetic_variant: "OTHER".into(), age_at_onset: 25,
            current_treatments: vec![],
            biomarker_values: vec![],
            country: "Japan".into(), language: "ja".into(),
        },
    ];

    let study = StudyCriteria {
        study_id: "NPC-GLOBAL-2026".into(),
        condition_code: "5C51.0".into(),
        target_variants: vec!["NPC1-I1061T".into()],
        age_range: (0, 18),
        min_participants: 3,
        excluded_treatments: vec![],
    };

    println!("Study: {} (Niemann-Pick type C, NPC1-I1061T variant)", study.study_id);
    println!("Minimum participants needed: {}\n", study.min_participants);

    let eligible: Vec<&RarePatient> = patients.iter()
        .filter(|p| check_eligibility(p, &study))
        .collect();

    let countries: Vec<&str> = eligible.iter()
        .map(|p| p.country.as_str())
        .collect();

    println!("Global pool: {} rare disease patients", patients.len());
    println!("Eligible for this study: {}", eligible.len());
    println!("Countries: {:?}", countries);
    println!("Viable study: {}\n",
        if eligible.len() >= study.min_participants { "YES" } else { "NO — need more patients" });

    println!("What the researcher sees:");
    println!("  ✓ '3 eligible patients exist in Japan, France, Germany'");
    println!("  ✗ Patient names: HIDDEN");
    println!("  ✗ Patient genetic data: HIDDEN");
    println!("  ✗ Patient treatment history: HIDDEN\n");

    println!("What happens next:");
    println!("  1. Eligible patients receive notification (via hyde)");
    println!("  2. Patient decides: participate or ignore");
    println!("  3. If ignore: no one knows they were notified");
    println!("  4. If participate: identity revealed ONLY to study team");
    println!("\nWithout qmed: these 3 patients would never find each other.");
    println!("Their hospitals are in different countries, different languages,");
    println!("different privacy regimes. The study would never happen.");
}
