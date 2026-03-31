//! Optimal donor-recipient matching (Argo layer).
//!
//! Solves the assignment problem: given an N×M compatibility score matrix,
//! find the optimal one-to-one matching that maximizes total score.
//!
//! In the encrypted protocol, this operates on CKKS ciphertext scores
//! from the PLAT layer.

/// Hungarian algorithm for maximum-weight bipartite matching.
/// Returns Vec<(donor_index, recipient_index, score)> — the optimal assignment.
/// Time complexity: O(N³) where N = max(donors, recipients).
pub fn hungarian_match(scores: &[Vec<f64>]) -> Vec<(usize, usize, f64)> {
    if scores.is_empty() {
        return vec![];
    }
    let n_donors = scores.len();
    let n_recipients = scores[0].len();
    let n = n_donors.max(n_recipients);

    // Pad to square matrix, negate for minimization
    let mut cost = vec![vec![0.0f64; n]; n];
    let max_score = scores
        .iter()
        .flat_map(|r| r.iter())
        .cloned()
        .fold(0.0f64, f64::max);
    for i in 0..n {
        for j in 0..n {
            if i < n_donors && j < n_recipients {
                cost[i][j] = max_score - scores[i][j]; // negate for minimization
            } else {
                cost[i][j] = max_score; // dummy entries
            }
        }
    }

    // Hungarian algorithm (Kuhn-Munkres)
    let mut u = vec![0.0f64; n + 1]; // potential for rows
    let mut v = vec![0.0f64; n + 1]; // potential for columns
    let mut p = vec![0usize; n + 1]; // assignment: p[j] = row assigned to column j
    let mut way = vec![0usize; n + 1];

    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![f64::INFINITY; n + 1];
        let mut used = vec![false; n + 1];

        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0usize;

            for j in 1..=n {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }

            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }

            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }

        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }

    let mut result = Vec::new();
    for j in 1..=n {
        let i = p[j] - 1;
        let jj = j - 1;
        if i < n_donors && jj < n_recipients && scores[i][jj] > 0.0 {
            result.push((i, jj, scores[i][jj]));
        }
    }
    result
}

/// Greedy matching on a score matrix.
/// Returns Vec<(donor_index, recipient_index, score)>.
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
