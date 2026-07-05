// Guardian-set PDA derivation and active-index detection.

use std::str::FromStr;

use eyre::{eyre, OptionExt, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

/// Pyth Solana Wormhole receiver program.
/// Source of truth: gmsol SDK `pyth/pull_oracle/wormhole/mod.rs` (WORMHOLE_PROGRAM_ID).
const WORMHOLE_PROGRAM_ID: &str = "HDwcJBJXjL9FpJ7UBsYBtaDjsBUhuLCUYoz3zr8SWWaQ";

pub fn program_id() -> Pubkey {
    Pubkey::from_str(WORMHOLE_PROGRAM_ID).expect("valid program id")
}

/// PDA for the guardian set at `index`: seeds = [b"GuardianSet", index.to_be_bytes()].
pub fn guardian_set_address(index: u32) -> Pubkey {
    Pubkey::find_program_address(&[b"GuardianSet", &index.to_be_bytes()], &program_id()).0
}

pub struct Detected {
    /// Highest existing guardian-set index (= the active set).
    pub active: u32,
    /// All indices in `1..=max_probe` whose account exists on the cluster.
    pub existing: Vec<u32>,
}

/// One `getMultipleAccounts` call over indices `1..=max_probe`; the highest existing
/// index is the active set (indices increment by +1 per rotation, newest is active).
/// Index 0 (the 2021 genesis set) is intentionally skipped: it is never used to sign
/// current VAAs, and skipping it keeps `existing` deterministic regardless of whether
/// the genesis account sits at the standard PDA.
pub fn detect(rpc_url: &str, max_probe: u32) -> Result<Detected> {
    let client = RpcClient::new(rpc_url.to_string());
    let indices: Vec<u32> = (1..=max_probe).collect();
    let addresses: Vec<Pubkey> = indices.iter().map(|&i| guardian_set_address(i)).collect();
    let accounts = client
        .get_multiple_accounts(&addresses)
        .map_err(|e| eyre!("getMultipleAccounts against {rpc_url}: {e}"))?;
    let existing: Vec<u32> = indices
        .iter()
        .zip(accounts.iter())
        .filter_map(|(&i, acc)| acc.as_ref().map(|_| i))
        .collect();
    let active = existing
        .iter()
        .copied()
        .max()
        .ok_or_eyre("no guardian-set accounts found on cluster")?;
    if existing.contains(&max_probe) {
        eprintln!(
            "warning: highest probed guardian-set index ({max_probe}) exists; a newer set \
             may be unprobed. Raise MAX_PROBE in xtask if a rotation is not detected."
        );
    }
    Ok(Detected { active, existing })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_known_guardian_set_addresses() {
        let cases = [
            (1u32, "8d9szTd157GKCLcxBqiLUgB7mek3v65rbsy2ErRyjwQ5"),
            (4, "5gxPdahvSzcKySxXxPuRXZZ9s6h8hZ88XDVKavWpaQGn"),
            (6, "HstYgN21fgNmutTVXjBw54n4ryvP3WrCFbMAjnbdbTzf"),
            (7, "6GaHgiaQg9Pg346xHq9m7vQ9rJtnH83gQKqJoiAxQa7D"),
        ];
        for (index, expected) in cases {
            assert_eq!(
                guardian_set_address(index).to_string(),
                expected,
                "index {index}"
            );
        }
    }
}
