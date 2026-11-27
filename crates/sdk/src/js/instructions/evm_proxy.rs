use super::{TransactionGroup, TransactionGroupOptions};
use crate::{
    js::evm_proxy::{parse_hex, EvmProxyCancelIntentArgs},
    serde::StringPubkey,
};
use gmsol_solana_utils::IntoAtomicGroup;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Arguments for a signed Cancel transaction.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct BuildEvmProxyCancelTransactionArgs {
    /// Signed fields and proxy deployment; signing-domain inputs are unnecessary.
    intent: EvmProxyCancelIntentArgs,
    /// Base58 transaction fee payer and intent submitter.
    payer: StringPubkey,
    /// Recent Solana blockhash in base58.
    recent_blockhash: String,
    /// Exactly 64 hex bytes encoding the compact ECDSA r || s signature.
    signature: String,
    /// Canonical recovery ID, zero or one.
    recovery_id: u8,
    /// Optional compute-unit price in micro-lamports.
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    /// Optional minimum priority fee in lamports.
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
    /// Standard transaction-group options, including memo and lookup tables.
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build a Cancel through the standard transaction group and compute-budget path.
#[wasm_bindgen]
pub fn build_evm_proxy_cancel_transaction(
    args: BuildEvmProxyCancelTransactionArgs,
) -> crate::Result<TransactionGroup> {
    let (program, intent) = args.intent.parse()?;
    let cancel = crate::builders::evm_proxy::Cancel::builder()
        .program(program.into())
        .payer(args.payer)
        .intent(intent)
        .signature(parse_hex(&args.signature, "signature")?)
        .recovery_id(args.recovery_id)
        .build()
        .into_atomic_group(&())?;
    let mut group = args.transaction_group.build();
    TransactionGroup::new(
        group.add(cancel)?.optimize(false),
        &args.recent_blockhash,
        args.compute_unit_price_micro_lamports,
        args.compute_unit_min_priority_lamports,
    )
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use gmsol_programs::{
        anchor_lang::{AnchorDeserialize, Discriminator},
        gmsol_evm_proxy::client::args,
    };
    use solana_sdk::{
        compute_budget::ComputeBudgetInstruction, hash::Hash, instruction::CompiledInstruction,
        pubkey::Pubkey, transaction::VersionedTransaction,
    };

    const PRIORITY_PRICE: u64 = 5_000;
    const SIGNED_NONCE: u64 = 9_007_199_254_740_993;

    fn transaction_args(payer: Pubkey) -> serde_json::Value {
        serde_json::json!({
            "intent": {
                "evm_address": hex::encode([0x17; 20]), "chain_id": hex::encode([1; 32]),
                "nonce": SIGNED_NONCE.to_string(), "deadline": u64::MAX.to_string()
            },
            "payer": payer.to_string(), "recent_blockhash": Hash::new_unique().to_string(),
            "signature": hex::encode([3; 64]), "recovery_id": 0,
            "compute_unit_price_micro_lamports": PRIORITY_PRICE,
            "compute_unit_min_priority_lamports": 0
        })
    }

    fn checked_transaction(
        group: TransactionGroup,
        payer: Pubkey,
        signer_count: usize,
    ) -> VersionedTransaction {
        let wire = group.serialize().unwrap().0;
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].len(), 1);
        let transaction: VersionedTransaction = bincode::deserialize(&wire[0][0]).unwrap();
        assert_eq!(transaction.signatures.len(), signer_count);
        let keys = transaction.message.static_account_keys();
        assert_eq!(keys[0], payer);
        let expected = ComputeBudgetInstruction::set_compute_unit_price(PRIORITY_PRICE);
        assert!(transaction.message.instructions().iter().any(|ix| keys
            [usize::from(ix.program_id_index)]
            == expected.program_id
            && ix.data == expected.data));
        transaction
    }

    fn proxy_instruction(transaction: &VersionedTransaction) -> &CompiledInstruction {
        let keys = transaction.message.static_account_keys();
        transaction
            .message
            .instructions()
            .iter()
            .find(|ix| {
                keys[usize::from(ix.program_id_index)] == gmsol_programs::gmsol_evm_proxy::ID
            })
            .expect("proxy instruction")
    }

    #[test]
    fn cancel_transaction_preserves_fee_options_payer_and_full_width_nonce() {
        let payer = Pubkey::new_unique();
        let input: BuildEvmProxyCancelTransactionArgs =
            serde_json::from_value(transaction_args(payer)).unwrap();
        let transaction =
            checked_transaction(build_evm_proxy_cancel_transaction(input).unwrap(), payer, 1);
        let instruction = proxy_instruction(&transaction);
        let keys = transaction.message.static_account_keys();
        assert_eq!(instruction.accounts.len(), 5);
        let program = gmsol_programs::gmsol_evm_proxy::ID;
        let authority = Pubkey::find_program_address(&[b"__event_authority"], &program).0;
        let authority_index = usize::from(instruction.accounts[3]);
        let program_index = usize::from(instruction.accounts[4]);
        assert_eq!(keys[authority_index], authority);
        assert_eq!(keys[program_index], program);
        assert!(!transaction.message.is_signer(authority_index));
        assert!(!transaction.message.is_signer(program_index));
        assert!(!transaction.message.is_maybe_writable(authority_index, None));
        assert!(!transaction.message.is_maybe_writable(program_index, None));
        let data =
            args::Cancel::deserialize(&mut &instruction.data[args::Cancel::DISCRIMINATOR.len()..])
                .unwrap();
        assert_eq!(data.nonce, SIGNED_NONCE);
    }
}
