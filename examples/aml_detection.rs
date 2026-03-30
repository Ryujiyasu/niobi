//! Example: Privacy-preserving anti-money laundering (AML) detection
//!
//! Current system: banks must check every international transfer
//! against sanctions lists and suspicious patterns. To do this
//! properly, they need to see the counterparty's customer data.
//! But sharing customer data across banks violates privacy laws.
//! Result: $2 trillion laundered annually, compliance costs $274B.
//!
//! With niobi: each bank encrypts customer transaction patterns
//! via hyde. plat (FHE) computes risk scores across banks without
//! exposing any customer's data. argo proves "this transaction is
//! clean" or flags it — without revealing account details.
//!
//! No bank sees another bank's customer data.
//! Regulators see proof of compliance, not raw data.

/// Bank's customer transaction pattern (encrypted via hyde).
struct CustomerPattern {
    bank_id: String,
    customer_hash: Vec<u8>,     // anonymous identifier
    avg_transaction_size: f64,   // encrypted
    transaction_frequency: f64,  // encrypted
    country_diversity: f64,      // encrypted (number of unique countries)
    high_risk_country_ratio: f64, // encrypted
}

/// AML risk score computed across multiple banks.
/// In production: plat performs FHE computation on encrypted patterns.
fn cross_bank_risk_score(patterns: &[CustomerPattern]) -> f64 {
    // Simulated FHE computation
    let mut risk = 0.0;

    for p in patterns {
        // Large transactions + high frequency + many countries = suspicious
        let size_risk = if p.avg_transaction_size > 50000.0 { 0.3 } else { 0.0 };
        let freq_risk = if p.transaction_frequency > 100.0 { 0.2 } else { 0.0 };
        let country_risk = p.high_risk_country_ratio * 0.5;

        risk += size_risk + freq_risk + country_risk;
    }

    (risk / patterns.len() as f64).min(1.0)
}

/// argo proof: "this transaction chain is clean" or "flagged"
struct AmlProof {
    transaction_hash: Vec<u8>,
    is_clean: bool,
    risk_score: f64,
    proof: Vec<u8>,
}

fn main() {
    println!("=== niobi Example: Anti-Money Laundering Detection ===\n");

    // Scenario: international transfer touches 3 banks
    let patterns = vec![
        CustomerPattern {
            bank_id: "MUFG-Tokyo".into(),
            customer_hash: vec![1, 2, 3],
            avg_transaction_size: 15000.0,
            transaction_frequency: 20.0,
            country_diversity: 3.0,
            high_risk_country_ratio: 0.0,
        },
        CustomerPattern {
            bank_id: "Deutsche-Frankfurt".into(),
            customer_hash: vec![4, 5, 6],
            avg_transaction_size: 80000.0,
            transaction_frequency: 150.0,
            country_diversity: 12.0,
            high_risk_country_ratio: 0.4,
        },
        CustomerPattern {
            bank_id: "HSBC-London".into(),
            customer_hash: vec![7, 8, 9],
            avg_transaction_size: 25000.0,
            transaction_frequency: 45.0,
            country_diversity: 5.0,
            high_risk_country_ratio: 0.1,
        },
    ];

    let risk = cross_bank_risk_score(&patterns);
    let is_flagged = risk > 0.3;

    println!("Transaction chain: {} banks across 3 countries", patterns.len());
    println!("Cross-bank risk score: {:.3}", risk);
    println!("Status: {}\n", if is_flagged { "⚠ FLAGGED" } else { "✓ CLEAN" });

    println!("What each bank sees:");
    println!("  ✓ Their own customer's pattern");
    println!("  ✗ Other banks' customer data: HIDDEN");
    println!("  ✗ Other banks' risk assessments: HIDDEN\n");

    println!("What the regulator sees:");
    println!("  ✓ argo proof: 'transaction is clean/flagged'");
    println!("  ✓ Aggregate risk score");
    println!("  ✗ Individual customer data: HIDDEN");
    println!("  ✗ Bank-specific patterns: HIDDEN\n");

    println!("Current cost of AML compliance: $274 billion/year globally");
    println!("Current laundering volume: $2 trillion/year");
    println!("Reason: banks can't share data to detect cross-bank patterns");
    println!("\nQuantum necessity:");
    println!("  - Global transaction graph: billions of edges");
    println!("  - Pattern detection across encrypted graph: combinatorial explosion");
    println!("  - Real-time flagging across time zones and jurisdictions");
}
