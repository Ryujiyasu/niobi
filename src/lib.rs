//! niobi: Privacy-preserving liver transplant matching
//!
//! Combines Hyde (encrypted data distribution), PLAT (FHE computation),
//! and Argo (optimal matching) to enable multi-hospital organ matching
//! without exposing patient data.

pub mod scoring;
pub mod matching;
pub mod protocol;
pub mod crypto;
pub mod privacy_protocol;
pub mod fhe_scoring;
pub mod zkp_compat;
pub mod exchange_chain;
pub mod annealing;
