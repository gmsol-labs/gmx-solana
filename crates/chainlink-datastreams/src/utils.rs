use anchor_lang::prelude::Pubkey;
use mock_chainlink_verifier::DEFAULT_VERIFIER_ACCOUNT_SEEDS;
use snap::raw::{Decoder, Encoder};

/// Find verifier account PDA.
pub fn find_verifier_account_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[DEFAULT_VERIFIER_ACCOUNT_SEEDS], program_id).0
}

/// Find config account PDA.
pub fn find_config_account_pda(report: &[u8], program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[&report[..32]], program_id).0
}

/// Compressor.
pub struct Compressor;

impl Compressor {
    /// Compress signed report.
    pub fn compress(data: &[u8]) -> snap::Result<Vec<u8>> {
        let mut encoder = Encoder::new();
        encoder.compress_vec(data)
    }

    /// Decompress signed report.
    pub fn decompress(compressed: &[u8]) -> snap::Result<Vec<u8>> {
        let mut decoder = Decoder::new();
        decoder.decompress_vec(compressed)
    }
}
