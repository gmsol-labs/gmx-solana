use gmsol_programs::anchor_lang::ToAccountMetas;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

pub use gmsol_utils::pubkey::{optional_address, DEFAULT_PUBKEY};

/// Change the `pubkey` of any readonly, non-signer [`AccountMeta`]
/// with the `pubkey` equal to the original program id to the new one.
///
/// This is a workaround since Anchor will automatically set optional accounts
/// to the Program ID of the program that defines them when they are `None`s,
/// if we use the same program but with different Program IDs, the optional
/// accounts will be set to the wrong addresses.
///
/// ## Warning
/// Use this function only if you fully understand the implications.
pub fn fix_optional_account_metas(
    accounts: impl ToAccountMetas,
    original: &Pubkey,
    current: &Pubkey,
) -> Vec<AccountMeta> {
    let mut metas = accounts.to_account_metas(None);
    if *original == *current {
        // No-op in this case.
        return metas;
    }
    metas.iter_mut().for_each(|meta| {
        if !meta.is_signer && !meta.is_writable && meta.pubkey == *original {
            // We consider it a `None` account. If it is not, please do not use this function.
            meta.pubkey = *current;
        }
    });
    metas
}
