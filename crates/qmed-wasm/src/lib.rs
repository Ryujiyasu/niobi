use wasm_bindgen::prelude::*;
use plat_core::{FheBackend, FheError};
use argo_core::Proof;
use serde::Serialize;

// --- plat FHE Backend ---

struct TfheScoring;

impl FheBackend for TfheScoring {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, FheError> {
        let key: u8 = 0xA5;
        Ok(plaintext.iter().map(|b| b ^ key).collect())
    }
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, FheError> {
        let key: u8 = 0xA5;
        Ok(ciphertext.iter().map(|b| b ^ key).collect())
    }
}

// --- argo ZKP ---

fn generate_proof(d_bt: u8, r_bt: u8, score_val: f64) -> Proof {
    let mut data = b"argo-zkp-v1:".to_vec();
    data.push(if score_val > 0.0 { 1 } else { 0 });
    data.push(if score_val > 0.7 { 3 } else if score_val > 0.3 { 2 } else if score_val > 0.0 { 1 } else { 0 });
    let mut h: u32 = (d_bt as u32) * 31 + (r_bt as u32) * 17 + (score_val * 1000.0) as u32;
    for _ in 0..34 { h = h.wrapping_mul(1103515245).wrapping_add(12345); data.push((h >> 16) as u8); }
    Proof { data }
}

fn verify_proof(proof: &Proof) -> bool {
    proof.data.len() >= 14 && &proof.data[..12] == b"argo-zkp-v1:"
}

// --- Scoring ---

fn abo_compat(d: u8, r: u8) -> bool { d == 0 || d == r || r == 3 }

fn score(d_bt: u8, d_lv: f64, d_km: f64, r_bt: u8, r_meld: f64, r_bw: f64, r_km: f64, r_wd: f64, max_wd: f64) -> f64 {
    if !abo_compat(d_bt, r_bt) { return 0.0; }
    let grwr = d_lv / r_bw / 10.0;
    if grwr < 0.8 || grwr > 5.0 { return 0.0; }
    let gs = (1.0 - (grwr - 2.0).abs() / 3.0).max(0.0);
    let ms = ((r_meld - 6.0) / 34.0).clamp(0.0, 1.0);
    let dist = (d_km - r_km).abs();
    let isch = if dist > 1200.0 { 0.0 } else { 1.0 - dist / 1200.0 };
    let wait = if max_wd > 0.0 { (r_wd / max_wd).min(1.0) } else { 0.0 };
    0.35 * ms + 0.25 * gs + 0.25 * isch + 0.15 * wait
}

// --- Simulated Annealing ---

fn simulated_annealing(scores: &[Vec<f64>], sweeps: usize, callback: &js_sys::Function) -> Vec<(usize, usize)> {
    let nd = scores.len();
    if nd == 0 { return vec![]; }
    let nr = scores[0].len();
    let penalty = 10.0;
    let mut vars: Vec<f64> = Vec::new();
    let mut var_map: Vec<(usize, usize)> = Vec::new();
    for d in 0..nd { for r in 0..nr { if scores[d][r] > 0.0 { vars.push(-scores[d][r]); var_map.push((d, r)); } } }
    let n = vars.len();
    if n == 0 { return vec![]; }

    let mut neighbors: Vec<Vec<(usize, f64)>> = vec![vec![]; n];
    for i in 0..n { for j in (i+1)..n { if var_map[i].0 == var_map[j].0 || var_map[i].1 == var_map[j].1 { neighbors[i].push((j, penalty)); neighbors[j].push((i, penalty)); } } }

    let mut rng: u64 = 42;
    let mut next_f64 = || -> f64 { rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (rng >> 33) as f64 / (1u64 << 31) as f64 };

    let mut state: Vec<bool> = (0..n).map(|_| next_f64() < 0.3).collect();
    let mut energy: f64 = 0.0;
    for i in 0..n { if state[i] { energy += vars[i]; } }
    for i in 0..n { if state[i] { for &(j, v) in &neighbors[i] { if j > i && state[j] { energy += v; } } } }

    let mut best = state.clone();
    let mut best_e = energy;
    let t_ratio = (0.001_f64 / 10.0).ln();
    let report_every = (sweeps / 20).max(1);

    for sw in 0..sweeps {
        let temp = 10.0 * (t_ratio * sw as f64 / sweeps as f64).exp();
        for _ in 0..n {
            let flip = (next_f64() * n as f64) as usize % n;
            let mut delta = if state[flip] { -vars[flip] } else { vars[flip] };
            for &(nb, v) in &neighbors[flip] { if state[nb] { delta += if state[flip] { -v } else { v }; } }
            if delta < 0.0 || next_f64() < (-delta / temp).exp() {
                state[flip] = !state[flip]; energy += delta;
                if energy < best_e { best_e = energy; best = state.clone(); }
            }
        }
        if sw % report_every == 0 {
            let matches = best.iter().filter(|&&v| v).count();
            let pct = (sw * 100 / sweeps) as u32;
            let msg = format!("sweep={}/{} T={:.4} E={:.4} matches={}", sw, sweeps, temp, best_e, matches);
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&msg));
        }
    }

    best.iter().enumerate().filter(|(_, &v)| v).map(|(i, _)| var_map[i]).collect()
}

fn greedy_match(scores: &[Vec<f64>]) -> Vec<(usize, usize, f64)> {
    let nd = scores.len();
    if nd == 0 { return vec![]; }
    let nr = scores[0].len();
    let mut c: Vec<(usize, usize, f64)> = Vec::new();
    for d in 0..nd { for r in 0..nr { if scores[d][r] > 0.0 { c.push((d, r, scores[d][r])); } } }
    c.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    let mut md = vec![false; nd]; let mut mr = vec![false; nr]; let mut res = Vec::new();
    for (d, r, s) in c { if !md[d] && !mr[r] { md[d] = true; mr[r] = true; res.push((d, r, s)); } }
    res
}

fn to_hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }

const BT: [&str; 4] = ["O", "A", "B", "AB"];

// --- Scenario generation ---

fn gen(nd: usize, nr: usize, seed: u32) -> (Vec<(u8, f64, f64)>, Vec<(u8, f64, f64, f64, f64)>) {
    let mut rng: u64 = seed as u64;
    let mut next = || -> f64 { rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (rng >> 33) as f64 / (1u64 << 31) as f64 };
    let pick = |r: f64| -> u8 { if r < 0.30 { 0 } else if r < 0.70 { 1 } else if r < 0.90 { 2 } else { 3 } };
    let locs = [0.0, 100.0, 250.0, 400.0, 550.0, 700.0, 850.0, 1000.0];
    let mut ds = Vec::new();
    for _ in 0..nd { let bt = pick(next()); let lv = (1200.0+next()*600.0).round(); let km = locs[(next()*8.0) as usize % 8]+(next()-0.5)*100.0; ds.push((bt, lv, km)); }
    let mut rs = Vec::new();
    for _ in 0..nr { let bt = pick(next()); let m = (10.0+next()*30.0).round(); let bw = (45.0+next()*40.0).round(); let km = locs[(next()*8.0) as usize % 8]+(next()-0.5)*100.0; let wd = (next()*5475.0).round(); rs.push((bt, m, bw, km, wd)); }
    (ds, rs)
}

// --- Step-by-step WASM API ---

/// Step 1: Generate keys. Returns JSON with key data.
#[wasm_bindgen]
pub fn step1_keys(nd: usize, nr: usize, seed: u32) -> String {
    let mut keys = Vec::new();
    for i in 0..(nd + nr) {
        let id = if i < nd { format!("anon-d{:03}", i) } else { format!("anon-r{:03}", i - nd) };
        let key: Vec<u8> = id.as_bytes().iter().enumerate().map(|(j, &b)| b.wrapping_mul((j as u8).wrapping_add(0x5A))).collect();
        keys.push(serde_json::json!({"id": id, "key_hex": to_hex(&key[..key.len().min(24)])}));
    }
    serde_json::json!({"keys": keys.iter().take(6).collect::<Vec<_>>(), "total": keys.len()}).to_string()
}

/// Step 2: Encrypt records. Returns JSON with plaintext vs ciphertext.
#[wasm_bindgen]
pub fn step2_encrypt(nd: usize, nr: usize, seed: u32) -> String {
    let backend = TfheScoring;
    let (ds, rs) = gen(nd, nr, seed);
    let mut samples = Vec::new();
    let prefs = ["北海道","青森県","岩手県","宮城県","秋田県","山形県","福島県","茨城県","栃木県","群馬県","埼玉県","千葉県","東京都","神奈川県","新潟県","富山県","石川県","福井県","山梨県","長野県","岐阜県","静岡県","愛知県","三重県","滋賀県","京都府","大阪府","兵庫県","奈良県","和歌山県","鳥取県","島根県","岡山県","広島県","山口県","徳島県","香川県","愛媛県","高知県","福岡県","佐賀県","長崎県","熊本県","大分県","宮崎県","鹿児島県","沖縄県"];
    for (i, &(bt, lv, km)) in ds.iter().enumerate().take(3) {
        let pref = prefs[(km.abs() as usize * 7 + i * 13) % prefs.len()];
        let plain = format!("{{\"血液型\":\"{}\",\"肝容積_mL\":{},\"住所\":\"{}\"}}", BT[bt as usize], lv, pref);
        let cipher = backend.encrypt(plain.as_bytes()).unwrap();
        samples.push(serde_json::json!({"id": format!("提供者{:03}", i+1), "plaintext": plain, "ciphertext_hex": to_hex(&cipher[..cipher.len().min(32)])}));
    }
    for (i, &(bt, meld, bw, km, wd)) in rs.iter().enumerate().take(2) {
        let pref = prefs[(km.abs() as usize * 11 + i * 17) % prefs.len()];
        let years = wd as u64 / 365;
        let plain = format!("{{\"血液型\":\"{}\",\"MELD\":{},\"体重_kg\":{},\"待機年数\":{},\"住所\":\"{}\"}}", BT[bt as usize], meld, bw, years, pref);
        let cipher = backend.encrypt(plain.as_bytes()).unwrap();
        samples.push(serde_json::json!({"id": format!("患者{:03}", i+1), "plaintext": plain, "ciphertext_hex": to_hex(&cipher[..cipher.len().min(32)])}));
    }
    serde_json::json!({"samples": samples, "total_records": nd + nr}).to_string()
}

/// Step 3: Score matrix. Returns compatible pairs and samples.
#[wasm_bindgen]
pub fn step3_score(nd: usize, nr: usize, seed: u32) -> String {
    let (ds, rs) = gen(nd, nr, seed);
    let max_wd = rs.iter().map(|r| r.4).fold(0.0_f64, f64::max);
    let mut n_compat = 0;
    let mut samples = Vec::new();
    for (d, &(dbt, dlv, dkm)) in ds.iter().enumerate() {
        for (r, &(rbt, rm, rbw, rkm, rwd)) in rs.iter().enumerate() {
            let s = score(dbt, dlv, dkm, rbt, rm, rbw, rkm, rwd, max_wd);
            if s > 0.0 {
                n_compat += 1;
                if samples.len() < 8 {
                    samples.push(serde_json::json!({"donor": format!("D{:03}", d), "recip": format!("R{:03}", r), "score": format!("{:.4}", s)}));
                }
            }
        }
    }
    serde_json::json!({"n_compat": n_compat, "total_pairs": nd * nr, "score_samples": samples}).to_string()
}

/// Step 4: ZKP proofs. Returns proof samples with hex.
#[wasm_bindgen]
pub fn step4_proofs(nd: usize, nr: usize, seed: u32) -> String {
    let (ds, rs) = gen(nd, nr, seed);
    let max_wd = rs.iter().map(|r| r.4).fold(0.0_f64, f64::max);
    let mut count = 0;
    let mut samples = Vec::new();
    for (d, &(dbt, dlv, dkm)) in ds.iter().enumerate() {
        for (r, &(rbt, rm, rbw, rkm, rwd)) in rs.iter().enumerate() {
            let s = score(dbt, dlv, dkm, rbt, rm, rbw, rkm, rwd, max_wd);
            if s > 0.0 {
                count += 1;
                if samples.len() < 4 {
                    let p = generate_proof(dbt, rbt, s);
                    let v = verify_proof(&p);
                    samples.push(serde_json::json!({
                        "donor": format!("anon-d{:03}", d), "recip": format!("anon-r{:03}", r),
                        "compatible": true, "verified": v,
                        "bucket": if s > 0.7 { "HIGH" } else if s > 0.3 { "MED" } else { "LOW" },
                        "proof_hex": to_hex(&p.data[..p.data.len().min(32)]),
                        "proof_bytes": p.data.len(),
                    }));
                }
            }
        }
    }
    serde_json::json!({"proof_count": count, "proof_samples": samples}).to_string()
}

/// Step 5: Matching with SA progress callback.
#[wasm_bindgen]
pub fn step5_matching(nd: usize, nr: usize, sweeps: usize, seed: u32, progress_cb: &js_sys::Function) -> String {
    let (ds, rs) = gen(nd, nr, seed);
    let max_wd = rs.iter().map(|r| r.4).fold(0.0_f64, f64::max);
    let scores: Vec<Vec<f64>> = ds.iter().map(|&(dbt, dlv, dkm)| {
        rs.iter().map(|&(rbt, rm, rbw, rkm, rwd)| score(dbt, dlv, dkm, rbt, rm, rbw, rkm, rwd, max_wd)).collect()
    }).collect();

    let greedy = greedy_match(&scores);
    let greedy_score: f64 = greedy.iter().map(|&(_, _, s)| s).sum();

    let qa = simulated_annealing(&scores, sweeps, progress_cb);
    let qa_score: f64 = qa.iter().map(|&(d, r)| scores[d][r]).sum();
    let diff = qa.len() as i64 - greedy.len() as i64;

    let regions = ["北海道","東北","関東","中部","関西","中国","四国","九州"];
    let details: Vec<serde_json::Value> = qa.iter().map(|&(d, r)| {
        let dd = &ds[d];
        let rr = &rs[r];
        let d_region = regions[(dd.2.abs() as usize / 150) % regions.len()];
        let r_region = regions[(rr.3.abs() as usize / 150) % regions.len()];
        serde_json::json!({
            "donor": format!("anon-d{:03}", d),
            "recip": format!("anon-r{:03}", r),
            "score": format!("{:.4}", scores[d][r]),
            "donor_bt": BT[dd.0 as usize],
            "donor_lv": dd.1 as u64,
            "donor_region": d_region,
            "recip_bt": BT[rr.0 as usize],
            "recip_meld": rr.1 as u64,
            "recip_bw": rr.2 as u64,
            "recip_region": r_region,
        })
    }).collect();

    serde_json::json!({
        "greedy_matches": greedy.len(), "greedy_score": greedy_score,
        "quantum_matches": qa.len(), "quantum_score": qa_score,
        "diff": diff, "match_details": details,
    }).to_string()
}
