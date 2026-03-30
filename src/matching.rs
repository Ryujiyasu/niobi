//! Optimal donor-recipient matching (Argo layer).
//!
//! Solves the assignment problem: given an N×M compatibility score matrix,
//! find the optimal one-to-one matching that maximizes total score.
//!
//! In the encrypted protocol, this operates on CKKS ciphertext scores
//! from the PLAT layer.

/// Greedy matching on a score matrix.
/// Returns Vec<(donor_index, recipient_index, score)>.
///
/// This is a simple greedy approach for initial prototyping.
/// TODO: Replace with Hungarian algorithm for optimal assignment.
pub fn greedy_match(scores: &[Vec<f64>]) -> Vec<(usize, usize, f64)> {
    if scores.is_empty() {
        return vec![];
    }

    let n_donors = scores.len();
    let n_recipients = scores[0].len();

    // Flatten and sort all (donor, recipient, score) triples by score descending
    let mut candidates: Vec<(usize, usize, f64)> = Vec::new();
    for (d, row) in scores.iter().enumerate() {
        for (r, &score) in row.iter().enumerate() {
            if score > 0.0 {
                candidates.push((d, r, score));
            }
        }
    }
    candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    let mut matched_donors = vec![false; n_donors];
    let mut matched_recipients = vec![false; n_recipients];
    let mut result = Vec::new();

    for (d, r, score) in candidates {
        if !matched_donors[d] && !matched_recipients[r] {
            matched_donors[d] = true;
            matched_recipients[r] = true;
            result.push((d, r, score));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greedy_simple() {
        let scores = vec![
            vec![0.9, 0.1],
            vec![0.2, 0.8],
        ];
        let matches = greedy_match(&scores);
        assert_eq!(matches.len(), 2);
        // Should match (0,0) and (1,1)
        assert!(matches.iter().any(|m| m.0 == 0 && m.1 == 0));
        assert!(matches.iter().any(|m| m.0 == 1 && m.1 == 1));
    }

    #[test]
    fn test_greedy_incompatible() {
        let scores = vec![
            vec![0.0, 0.5],
            vec![0.0, 0.0],
        ];
        let matches = greedy_match(&scores);
        // Only one valid match: donor 0 -> recipient 1
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], (0, 1, 0.5));
    }

    #[test]
    fn test_empty() {
        let matches = greedy_match(&[]);
        assert!(matches.is_empty());
    }
}
