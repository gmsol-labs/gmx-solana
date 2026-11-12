//! EVM proxy Config instructions and checked initialization.

use std::{future::Future, ops::Deref};

use gmsol_programs::{
    anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas},
    gmsol_evm_proxy::client::{accounts, args},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{
    bpf_loader_upgradeable, instruction::Instruction, pubkey::Pubkey, signature::Signature,
    signer::Signer, system_program,
};

use crate::{pda::find_proxy_config_address, Client};

fn instruction(
    program_id: Pubkey,
    accounts: impl ToAccountMetas,
    args: impl InstructionData,
) -> Instruction {
    Instruction {
        program_id,
        accounts: accounts.to_account_metas(None),
        data: args.data(),
    }
}

/// Build the one-time Config initialization instruction.
pub fn create_config_instruction(
    program_id: Pubkey,
    payer: Pubkey,
    upgrade_authority: Pubkey,
    store: Pubkey,
    genesis_hash: [u8; 32],
    first_admin: Pubkey,
) -> Instruction {
    instruction(
        program_id,
        accounts::CreateConfig {
            payer,
            upgrade_authority,
            config: find_proxy_config_address(&program_id).0,
            program: program_id,
            program_data: bpf_loader_upgradeable::get_program_data_address(&program_id),
            store,
            system_program: system_program::ID,
        },
        args::CreateConfig {
            genesis_hash,
            first_admin,
        },
    )
}

/// Build one current-admin chain allowlist mutation.
pub fn set_chain_allowed_instruction(
    program_id: Pubkey,
    admin: Pubkey,
    chain_id: [u8; 32],
    allowed: bool,
) -> Instruction {
    instruction(
        program_id,
        accounts::SetChainAllowed {
            admin,
            config: find_proxy_config_address(&program_id).0,
        },
        args::SetChainAllowed { chain_id, allowed },
    )
}

/// Build one current-admin pause mutation.
pub fn set_paused_instruction(program_id: Pubkey, admin: Pubkey, paused: bool) -> Instruction {
    instruction(
        program_id,
        accounts::SetPaused {
            admin,
            config: find_proxy_config_address(&program_id).0,
        },
        args::SetPaused { paused },
    )
}

/// Build one current-admin handover proposal or replacement.
pub fn propose_admin_instruction(
    program_id: Pubkey,
    admin: Pubkey,
    next_admin: Pubkey,
) -> Instruction {
    instruction(
        program_id,
        accounts::ProposeAdmin {
            admin,
            config: find_proxy_config_address(&program_id).0,
        },
        args::ProposeAdmin { next_admin },
    )
}

/// Build pending-admin acceptance; the pending admin must sign the transaction.
pub fn accept_admin_instruction(program_id: Pubkey, next_admin: Pubkey) -> Instruction {
    instruction(
        program_id,
        accounts::AcceptAdmin {
            next_admin,
            config: find_proxy_config_address(&program_id).0,
        },
        args::AcceptAdmin {},
    )
}

/// Account fields needed to verify immutable initialization values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReadback {
    /// Bound Store.
    pub store: Pubkey,
    /// Caller-supplied hash persisted by the program.
    pub genesis_hash: [u8; 32],
    /// Admin observed at readback time. It may have changed after creation.
    pub current_admin: Pubkey,
}

/// Result of a transaction-confirmed initialization and separate readback.
#[derive(Debug)]
pub enum ConfigInitialization {
    /// Immutable fields matched the requested values.
    Verified {
        /// Confirmed transaction signature.
        signature: Signature,
        /// Current admin observed at readback time, not asserted to be the first admin.
        current_admin: Pubkey,
    },
    /// Transaction was confirmed but readback could not verify immutable fields.
    VerificationIncomplete {
        /// Confirmed transaction signature. Inspect it before any further action.
        signature: Signature,
        /// Readback failure or mismatch.
        reason: String,
    },
}

async fn initialize_with<Get, GetFuture, Send, SendFuture, Read, ReadFuture>(
    expected_hash: [u8; 32],
    store: Pubkey,
    get_genesis: Get,
    send: Send,
    read: Read,
) -> crate::Result<ConfigInitialization>
where
    Get: FnOnce() -> GetFuture,
    GetFuture: Future<Output = crate::Result<String>>,
    Send: FnOnce() -> SendFuture,
    SendFuture: Future<Output = crate::Result<Signature>>,
    Read: FnOnce() -> ReadFuture,
    ReadFuture: Future<Output = crate::Result<ConfigReadback>>,
{
    let encoded = get_genesis().await?;
    let decoded = bs58::decode(encoded)
        .into_vec()
        .map_err(|_| crate::Error::custom("RPC returned an invalid genesis hash"))?;
    let actual: [u8; 32] = decoded
        .try_into()
        .map_err(|_| crate::Error::custom("RPC genesis hash must contain 32 bytes"))?;
    if actual != expected_hash {
        return Err(crate::Error::custom(
            "supplied genesis hash differs from RPC",
        ));
    }

    let signature = send().await?;
    let verification = read().await.and_then(|account| {
        if account.store != store || account.genesis_hash != expected_hash {
            Err(crate::Error::custom(
                "Config immutable fields differ from initialization",
            ))
        } else {
            Ok(account.current_admin)
        }
    });
    Ok(match verification {
        Ok(current_admin) => ConfigInitialization::Verified {
            signature,
            current_admin,
        },
        Err(error) => ConfigInitialization::VerificationIncomplete {
            signature,
            reason: error.to_string(),
        },
    })
}

impl<C: Deref<Target = impl Signer> + Clone> Client<C> {
    /// Wrap a proxy instruction in this client's configured transaction builder.
    pub fn evm_proxy_instruction(&self, ix: Instruction) -> TransactionBuilder<'_, C> {
        self.program(ix.program_id)
            .transaction()
            .accounts(ix.accounts)
            .args(ix.data)
    }

    fn create_config_transaction<'a>(
        &'a self,
        store: Pubkey,
        genesis_hash: [u8; 32],
        first_admin: Pubkey,
        upgrade_authority: &'a dyn Signer,
    ) -> TransactionBuilder<'a, C> {
        let ix = create_config_instruction(
            gmsol_programs::gmsol_evm_proxy::ID,
            self.payer(),
            upgrade_authority.pubkey(),
            store,
            genesis_hash,
            first_admin,
        );
        let builder = self.evm_proxy_instruction(ix);
        if upgrade_authority.pubkey() == self.payer() {
            builder
        } else {
            builder.signer(upgrade_authority)
        }
    }

    /// Fetch and decode the full singleton proxy Config from this client's RPC endpoint.
    pub async fn proxy_config_account(
        &self,
    ) -> crate::Result<gmsol_programs::gmsol_evm_proxy::accounts::Config> {
        let program_id = gmsol_programs::gmsol_evm_proxy::ID;
        let address = find_proxy_config_address(&program_id).0;
        let account = self
            .rpc()
            .get_account(&address)
            .await
            .map_err(crate::Error::custom)?;
        if account.owner != program_id {
            return Err(crate::Error::custom("Config has the wrong owner"));
        }
        gmsol_programs::gmsol_evm_proxy::accounts::Config::try_deserialize(
            &mut account.data.as_slice(),
        )
        .map_err(Into::into)
    }

    /// Read immutable Config fields and the admin currently observed on this RPC.
    pub async fn proxy_config(&self) -> crate::Result<ConfigReadback> {
        let config = self.proxy_config_account().await?;
        Ok(ConfigReadback {
            store: config.store,
            genesis_hash: config.genesis_hash,
            current_admin: config.admin,
        })
    }

    /// Initialize Config after a same-endpoint RPC genesis preflight, then verify readback.
    ///
    /// The program stores the supplied hash; the RPC check is client-side. A confirmed
    /// transaction is never retried because of readback failure. Admin may legitimately
    /// change before readback, so this API reports it without claiming a handover audit.
    pub async fn initialize_proxy_config(
        &self,
        store: Pubkey,
        genesis_hash: [u8; 32],
        first_admin: Pubkey,
        upgrade_authority: &dyn Signer,
    ) -> crate::Result<ConfigInitialization> {
        initialize_with(
            genesis_hash,
            store,
            || async {
                self.rpc()
                    .get_genesis_hash()
                    .await
                    .map(|hash| hash.to_string())
                    .map_err(crate::Error::custom)
            },
            || async {
                self.create_config_transaction(store, genesis_hash, first_admin, upgrade_authority)
                    .send()
                    .await
                    .map_err(Into::into)
            },
            || self.proxy_config(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::Arc};

    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::{hash::Hash, signature::Keypair};

    use super::*;

    #[tokio::test]
    async fn preflight_rejections_send_nothing() {
        let hash = [7u8; 32];
        let store = Pubkey::new_unique();
        for response in [
            Ok(bs58::encode([8u8; 32]).into_string()),
            Ok("not a base58 hash".to_string()),
            Ok(bs58::encode([7u8; 31]).into_string()),
            Err(crate::Error::custom("RPC unavailable")),
        ] {
            let sends = Cell::new(0);
            let reads = Cell::new(0);
            let result = initialize_with(
                hash,
                store,
                || async { response },
                || async {
                    sends.set(sends.get() + 1);
                    Ok(Signature::new_unique())
                },
                || async {
                    reads.set(reads.get() + 1);
                    unreachable!("readback must not run after rejected preflight")
                },
            )
            .await;
            assert!(result.is_err());
            assert_eq!(sends.get(), 0);
            assert_eq!(reads.get(), 0);
        }
    }

    #[tokio::test]
    async fn confirmed_send_and_readback_are_separate() {
        let hash = [7u8; 32];
        let store = Pubkey::new_unique();
        let later_admin = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let sends = Cell::new(0);
        let reads = Cell::new(0);
        let result = initialize_with(
            hash,
            store,
            || async { Ok(bs58::encode(hash).into_string()) },
            || async {
                sends.set(sends.get() + 1);
                Ok(signature)
            },
            || async {
                reads.set(reads.get() + 1);
                Ok(ConfigReadback {
                    store,
                    genesis_hash: hash,
                    current_admin: later_admin,
                })
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            ConfigInitialization::Verified {
                signature: actual,
                current_admin,
            } if actual == signature && current_admin == later_admin
        ));
        assert_eq!(sends.get(), 1);
        assert_eq!(reads.get(), 1);
    }

    #[tokio::test]
    async fn failed_readback_keeps_confirmed_signature_and_never_resends() {
        let hash = [7u8; 32];
        let store = Pubkey::new_unique();
        for response in [
            Err(crate::Error::custom("readback unavailable")),
            Ok(ConfigReadback {
                store: Pubkey::new_unique(),
                genesis_hash: hash,
                current_admin: Pubkey::new_unique(),
            }),
            Ok(ConfigReadback {
                store,
                genesis_hash: [8u8; 32],
                current_admin: Pubkey::new_unique(),
            }),
        ] {
            let sends = Cell::new(0);
            let signature = Signature::new_unique();
            let result = initialize_with(
                hash,
                store,
                || async { Ok(bs58::encode(hash).into_string()) },
                || async {
                    sends.set(sends.get() + 1);
                    Ok(signature)
                },
                || async { response },
            )
            .await
            .unwrap();
            assert!(matches!(
                result,
                ConfigInitialization::VerificationIncomplete {
                    signature: actual,
                    ..
                } if actual == signature
            ));
            assert_eq!(sends.get(), 1);
        }
    }

    #[test]
    fn instruction_builders_use_singleton_and_full_width_chain_id() {
        let program = gmsol_programs::gmsol_evm_proxy::ID;
        let config = find_proxy_config_address(&program).0;
        let admin = Pubkey::new_unique();
        let mut chain_id = [0u8; 32];
        chain_id[0] = 1;
        chain_id[31] = 2;
        let create = create_config_instruction(
            program,
            Pubkey::new_unique(),
            admin,
            Pubkey::new_unique(),
            [3; 32],
            admin,
        );
        assert_eq!(create.program_id, program);
        assert!(create
            .accounts
            .iter()
            .any(|meta| meta.pubkey == config && meta.is_writable));
        assert!(create
            .accounts
            .iter()
            .any(|meta| meta.pubkey == admin && meta.is_signer));
        let change = set_chain_allowed_instruction(program, admin, chain_id, true);
        assert_eq!(change.program_id, program);
        assert!(change
            .accounts
            .iter()
            .any(|meta| meta.pubkey == config && meta.is_writable));
        assert!(change.data.windows(32).any(|bytes| bytes == chain_id));
    }

    #[test]
    fn payer_as_upgrade_authority_signs_one_slot() {
        let payer = Arc::new(Keypair::new());
        let client = Client::new(Cluster::Localnet, payer.clone()).unwrap();
        let tx = client
            .create_config_transaction(Pubkey::new_unique(), [5; 32], payer.pubkey(), &*payer)
            .signed_transaction_with_blockhash_and_options(
                Hash::new_unique(),
                true,
                None,
                None,
                |_| Ok(()),
            )
            .expect("same-wallet payer and authority must sign once");
        assert_eq!(tx.signatures.len(), 1);
    }

    #[test]
    fn distinct_upgrade_authority_and_payer_both_sign() {
        let payer = Arc::new(Keypair::new());
        let authority = Keypair::new();
        let client = Client::new(Cluster::Localnet, payer).unwrap();
        let tx = client
            .create_config_transaction(
                Pubkey::new_unique(),
                [5; 32],
                authority.pubkey(),
                &authority,
            )
            .signed_transaction_with_blockhash_and_options(
                Hash::new_unique(),
                true,
                None,
                None,
                |_| Ok(()),
            )
            .expect("distinct payer and authority must both sign");
        assert_eq!(tx.signatures.len(), 2);
    }
}

#[cfg(test)]
mod builder_tests {
    use super::*;

    #[test]
    fn mutation_builders_keep_config_writable_and_require_accepting_signer() {
        let program = gmsol_programs::gmsol_evm_proxy::ID;
        let config = find_proxy_config_address(&program).0;
        let admin = Pubkey::new_unique();
        let next_admin = Pubkey::new_unique();
        let pause = set_paused_instruction(program, admin, false);
        let propose = propose_admin_instruction(program, admin, next_admin);
        let accept = accept_admin_instruction(program, next_admin);
        for ix in [&pause, &propose, &accept] {
            assert_eq!(ix.program_id, program);
            assert!(ix
                .accounts
                .iter()
                .any(|meta| meta.pubkey == config && meta.is_writable));
        }
        assert!(accept
            .accounts
            .iter()
            .any(|meta| meta.pubkey == next_admin && meta.is_signer));
    }
}
