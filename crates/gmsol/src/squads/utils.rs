use anchor_client::solana_client::nonblocking::rpc_client::RpcClient;
use anchor_lang::{AccountDeserialize, AnchorDeserialize, AnchorSerialize};
use solana_sdk::{message::VersionedMessage, pubkey::Pubkey};

use super::{
    small_vec::SmallVec,
    squads_multisig_v4::{
        accounts::Multisig,
        types::{
            MultisigCompiledInstruction, MultisigMessageAddressTableLookup, VaultTransactionMessage,
        },
    },
};

pub(super) async fn get_multisig(
    rpc_client: &RpcClient,
    multisig_key: &Pubkey,
) -> crate::Result<Multisig> {
    let multisig_account = rpc_client
        .get_account(multisig_key)
        .await
        .map_err(anchor_client::ClientError::from)?;

    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())?;

    Ok(multisig)
}

impl VaultTransactionMessage {
    /// Returns true if the account at the specified index is a part of static `account_keys` and was requested to be writable.
    pub fn is_static_writable_index(&self, key_index: usize) -> bool {
        let num_account_keys = self.account_keys.len();
        let num_signers = usize::from(self.num_signers);
        let num_writable_signers = usize::from(self.num_writable_signers);
        let num_writable_non_signers = usize::from(self.num_writable_non_signers);

        if key_index >= num_account_keys {
            // `index` is not a part of static `account_keys`.
            return false;
        }

        if key_index < num_writable_signers {
            // `index` is within the range of writable signer keys.
            return true;
        }

        if key_index >= num_signers {
            // `index` is within the range of non-signer keys.
            let index_into_non_signers = key_index.saturating_sub(num_signers);
            // Whether `index` is within the range of writable non-signer keys.
            return index_into_non_signers < num_writable_non_signers;
        }

        false
    }

    /// Returns true if the account at the specified index was requested to be a signer.
    pub fn is_signer_index(&self, key_index: usize) -> bool {
        key_index < usize::from(self.num_signers)
    }
}

impl TryFrom<TransactionMessage> for VaultTransactionMessage {
    type Error = crate::Error;

    fn try_from(msg: TransactionMessage) -> Result<Self, Self::Error> {
        Ok(Self {
            num_signers: msg.num_signers,
            num_writable_signers: msg.num_writable_signers,
            num_writable_non_signers: msg.num_writable_non_signers,
            account_keys: msg.account_keys.into(),
            instructions: Vec::<CompiledInstruction>::from(msg.instructions)
                .into_iter()
                .map(|ix| MultisigCompiledInstruction {
                    program_id_index: ix.program_id_index,
                    account_indexes: ix.account_indexes.into(),
                    data: ix.data.into(),
                })
                .collect(),
            address_table_lookups: Vec::<MessageAddressTableLookup>::from(
                msg.address_table_lookups,
            )
            .into_iter()
            .map(|atl| MultisigMessageAddressTableLookup {
                account_key: atl.account_key,
                writable_indexes: atl.writable_indexes.into(),
                readonly_indexes: atl.readonly_indexes.into(),
            })
            .collect(),
        })
    }
}

/// Unvalidated instruction data, must be treated as untrusted.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct TransactionMessage {
    /// The number of signer pubkeys in the account_keys vec.
    pub num_signers: u8,
    /// The number of writable signer pubkeys in the account_keys vec.
    pub num_writable_signers: u8,
    /// The number of writable non-signer pubkeys in the account_keys vec.
    pub num_writable_non_signers: u8,
    /// The list of unique account public keys (including program IDs) that will be used in the provided instructions.
    pub account_keys: SmallVec<u8, Pubkey>,
    /// The list of instructions to execute.
    pub instructions: SmallVec<u8, CompiledInstruction>,
    /// List of address table lookups used to load additional accounts
    /// for this transaction.
    pub address_table_lookups: SmallVec<u8, MessageAddressTableLookup>,
}

// Concise serialization schema for instructions that make up transaction.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CompiledInstruction {
    pub program_id_index: u8,
    /// Indices into the tx's `account_keys` list indicating which accounts to pass to the instruction.
    pub account_indexes: SmallVec<u8, u8>,
    /// Instruction data.
    pub data: SmallVec<u16, u8>,
}

/// Address table lookups describe an on-chain address lookup table to use
/// for loading more readonly and writable accounts in a single tx.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MessageAddressTableLookup {
    /// Address lookup table account key
    pub account_key: Pubkey,
    /// List of indexes used to load writable account addresses
    pub writable_indexes: SmallVec<u8, u8>,
    /// List of indexes used to load readonly account addresses
    pub readonly_indexes: SmallVec<u8, u8>,
}

pub(super) fn versioned_message_to_transaction_message(
    message: &VersionedMessage,
) -> TransactionMessage {
    match message {
        VersionedMessage::Legacy(message) => {
            let num_accounts = message.account_keys.len() as u8;
            let num_signers = message.header.num_required_signatures;
            let num_non_signers = num_accounts - num_signers;
            let instructions = message
                .instructions
                .iter()
                .map(|ix| CompiledInstruction {
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
                .map(|ix| CompiledInstruction {
                    program_id_index: ix.program_id_index,
                    account_indexes: ix.accounts.clone().into(),
                    data: ix.data.clone().into(),
                })
                .collect::<Vec<_>>();
            let address_table_lookups = message
                .address_table_lookups
                .iter()
                .map(|atl| MessageAddressTableLookup {
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
