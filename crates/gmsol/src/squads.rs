use std::{future::Future, ops::Deref};

use anchor_client::anchor_lang;
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{
    hash::Hash,
    instruction::{CompiledInstruction, Instruction},
    message::{
        v0::{Message, MessageAddressTableLookup},
        MessageHeader, VersionedMessage,
    },
    pubkey::Pubkey,
    signer::Signer,
};
use squads_multisig::{
    client::{
        get_multisig, ProposalCreateAccounts, ProposalCreateArgs, ProposalCreateData,
        VaultTransactionCreateAccounts, VaultTransactionCreateArgs, VaultTransactionCreateData,
    },
    pda::{get_proposal_pda, get_transaction_pda},
    squads_multisig_program::{self, VaultTransaction},
    state::TransactionMessage,
};

pub use squads_multisig::pda::get_vault_pda;

/// Squads Vault Transaction.
pub struct SquadsVaultTransaction(VaultTransaction);

impl anchor_lang::Discriminator for SquadsVaultTransaction {
    const DISCRIMINATOR: [u8; 8] =
        <VaultTransaction as squads_multisig::anchor_lang::Discriminator>::DISCRIMINATOR;
}

impl anchor_lang::AccountDeserialize for SquadsVaultTransaction {
    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let inner = <VaultTransaction as squads_multisig::anchor_lang::AccountDeserialize>::try_deserialize_unchecked(buf).map_err(|_err| {
            anchor_lang::error::ErrorCode::AccountDidNotDeserialize
        })?;

        Ok(Self(inner))
    }
}

impl SquadsVaultTransaction {
    /// Convert to transaction message.
    pub fn to_message(&self) -> Message {
        let message = &self.0.message;
        let instructions = message
            .instructions
            .iter()
            .map(|ix| CompiledInstruction {
                program_id_index: ix.program_id_index,
                accounts: ix.account_indexes.clone(),
                data: ix.data.clone(),
            })
            .collect();
        let address_table_lookups = message
            .address_table_lookups
            .iter()
            .map(|atl| MessageAddressTableLookup {
                account_key: atl.account_key,
                writable_indexes: atl.writable_indexes.clone(),
                readonly_indexes: atl.readonly_indexes.clone(),
            })
            .collect();
        let num_non_signers = message.account_keys.len() as u8 - message.num_signers;
        Message {
            header: MessageHeader {
                num_required_signatures: message.num_signers,
                num_readonly_signed_accounts: message.num_signers - message.num_writable_signers,
                num_readonly_unsigned_accounts: num_non_signers - message.num_writable_non_signers,
            },
            account_keys: message.account_keys.clone(),
            recent_blockhash: Hash::default(),
            instructions,
            address_table_lookups,
        }
    }
}

/// Squads Multisig Ops.
pub trait SquadsOps<C> {
    /// Create Vault Transaction with the given transaction index.
    fn squads_create_vault_transaction_with_index(
        &self,
        multisig: &Pubkey,
        vault_index: u8,
        transaction_index: u64,
        message: &VersionedMessage,
        memo: Option<String>,
        draft: bool,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>>;

    /// Create Vault Transaction with next transaction index.
    fn squads_create_vault_transaction(
        &self,
        multisig: &Pubkey,
        vault_index: u8,
        message: &VersionedMessage,
        memo: Option<String>,
        draft: bool,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C, Pubkey>>>;
}

impl<C: Deref<Target = impl Signer> + Clone> SquadsOps<C> for crate::Client<C> {
    fn squads_create_vault_transaction_with_index(
        &self,
        multisig: &Pubkey,
        vault_index: u8,
        transaction_index: u64,
        message: &VersionedMessage,
        memo: Option<String>,
        draft: bool,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        use squads_multisig::{
            anchor_lang::{AnchorSerialize, InstructionData, ToAccountMetas},
            squads_multisig_program::ID,
        };

        let payer = self.payer();
        let transaction_pda = get_transaction_pda(multisig, transaction_index, Some(&ID)).0;
        let proposal_pda = get_proposal_pda(multisig, transaction_index, Some(&ID)).0;

        let transaction_message = versioned_message_to_transaction_message(message);
        let rpc = self.store_transaction().pre_instructions(vec![
            Instruction {
                program_id: ID,
                accounts: VaultTransactionCreateAccounts {
                    creator: payer,
                    rent_payer: payer,
                    transaction: transaction_pda,
                    multisig: *multisig,
                    system_program: solana_sdk::system_program::id(),
                }
                .to_account_metas(Some(false)),
                data: VaultTransactionCreateData {
                    args: VaultTransactionCreateArgs {
                        ephemeral_signers: 0,
                        vault_index,
                        memo,
                        transaction_message: transaction_message.try_to_vec()?,
                    },
                }
                .data(),
            },
            Instruction {
                program_id: ID,
                accounts: ProposalCreateAccounts {
                    creator: payer,
                    rent_payer: payer,
                    proposal: proposal_pda,
                    multisig: *multisig,
                    system_program: solana_sdk::system_program::id(),
                }
                .to_account_metas(Some(false)),
                data: ProposalCreateData {
                    args: ProposalCreateArgs {
                        draft,
                        transaction_index,
                    },
                }
                .data(),
            },
        ]);
        Ok(rpc.output(transaction_pda))
    }

    async fn squads_create_vault_transaction(
        &self,
        multisig: &Pubkey,
        vault_index: u8,
        message: &VersionedMessage,
        memo: Option<String>,
        draft: bool,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let multisig_data = get_multisig(&self.store_program().rpc(), multisig)
            .await
            .map_err(crate::Error::unknown)?;

        self.squads_create_vault_transaction_with_index(
            multisig,
            vault_index,
            multisig_data.transaction_index + 1,
            message,
            memo,
            draft,
        )
    }
}

fn versioned_message_to_transaction_message(message: &VersionedMessage) -> TransactionMessage {
    match message {
        VersionedMessage::Legacy(message) => {
            let num_accounts = message.account_keys.len() as u8;
            let num_signers = message.header.num_required_signatures;
            let num_non_signers = num_accounts - num_signers;
            let instructions = message
                .instructions
                .iter()
                .map(|ix| squads_multisig_program::CompiledInstruction {
                    program_id_index: ix.program_id_index,
                    account_indexes: ix.accounts.clone().into(),
                    data: ix.data.clone().into(),
                })
                .collect::<Vec<_>>();
            TransactionMessage {
                num_signers,
                num_writable_signers: num_signers - message.header.num_readonly_signed_accounts,
                num_writable_non_signers: num_non_signers
                    - message.header.num_readonly_unsigned_accounts,
                account_keys: message.account_keys.clone().into(),
                instructions: instructions.into(),
                address_table_lookups: Vec::default().into(),
            }
        }
        VersionedMessage::V0(message) => {
            let num_accounts = message.account_keys.len() as u8;
            let num_signers = message.header.num_required_signatures;
            let num_non_signers = num_accounts - num_signers;
            let instructions = message
                .instructions
                .iter()
                .map(|ix| squads_multisig_program::CompiledInstruction {
                    program_id_index: ix.program_id_index,
                    account_indexes: ix.accounts.clone().into(),
                    data: ix.data.clone().into(),
                })
                .collect::<Vec<_>>();
            let address_table_lookups = message
                .address_table_lookups
                .iter()
                .map(|atl| squads_multisig_program::MessageAddressTableLookup {
                    account_key: atl.account_key,
                    writable_indexes: atl.writable_indexes.clone().into(),
                    readonly_indexes: atl.readonly_indexes.clone().into(),
                })
                .collect::<Vec<_>>();
            TransactionMessage {
                num_signers,
                num_writable_signers: num_signers - message.header.num_readonly_signed_accounts,
                num_writable_non_signers: num_non_signers
                    - message.header.num_readonly_unsigned_accounts,
                account_keys: message.account_keys.clone().into(),
                instructions: instructions.into(),
                address_table_lookups: address_table_lookups.into(),
            }
        }
    }
}
