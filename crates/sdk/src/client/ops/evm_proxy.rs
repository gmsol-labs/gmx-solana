//! EVM proxy Config operations and immutable initialization verification.

use std::{future::Future, ops::Deref};

use gmsol_evm_proxy_intent::ChainId;
use gmsol_programs::{
    anchor_lang::AccountDeserialize,
    gmsol_evm_proxy::client::{accounts, args},
};
use gmsol_solana_utils::{transaction_builder::TransactionBuilder, IntoAtomicGroup, ProgramExt};
use solana_sdk::{
    bpf_loader_upgradeable, instruction::Instruction, pubkey::Pubkey, signature::Signature,
    signer::Signer, system_program,
};

use crate::{
    builders::evm_proxy::{Cancel, EvmProxyProgram},
    pda::find_proxy_config_address,
    Client,
};

/// Build the one-time Config initialization instruction.
pub fn create_config_instruction(
    program: &EvmProxyProgram,
    payer: Pubkey,
    upgrade_authority: Pubkey,
    store: Pubkey,
    genesis_hash: [u8; 32],
    first_admin: Pubkey,
) -> Instruction {
    let program_id = *program.id;
    program
        .anchor_instruction(args::CreateConfig { genesis_hash })
        .anchor_accounts(
            accounts::CreateConfig {
                payer,
                upgrade_authority,
                config: find_proxy_config_address(&program_id).0,
                proxy_program: program_id,
                program_data: bpf_loader_upgradeable::get_program_data_address(&program_id),
                store,
                first_admin,
                system_program: system_program::ID,
                event_authority: program.find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build one current-admin chain allowlist mutation.
pub fn set_chain_allowed_instruction(
    program: &EvmProxyProgram,
    admin: Pubkey,
    chain_id: [u8; 32],
    allowed: bool,
) -> Instruction {
    let program_id = *program.id;
    program
        .anchor_instruction(args::SetChainAllowed { chain_id, allowed })
        .anchor_accounts(
            accounts::SetChainAllowed {
                admin,
                config: find_proxy_config_address(&program_id).0,
                event_authority: program.find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build one current-admin pause mutation.
pub fn set_paused_instruction(
    program: &EvmProxyProgram,
    admin: Pubkey,
    paused: bool,
) -> Instruction {
    let program_id = *program.id;
    program
        .anchor_instruction(args::SetPaused { paused })
        .anchor_accounts(
            accounts::SetPaused {
                admin,
                config: find_proxy_config_address(&program_id).0,
                event_authority: program.find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build a current-admin handover proposal or replacement, or a withdrawal
/// when `next_admin` is the current admin.
pub fn transfer_admin_instruction(
    program: &EvmProxyProgram,
    admin: Pubkey,
    next_admin: Pubkey,
) -> Instruction {
    let program_id = *program.id;
    program
        .anchor_instruction(args::TransferAdmin {})
        .anchor_accounts(
            accounts::TransferAdmin {
                admin,
                config: find_proxy_config_address(&program_id).0,
                next_admin,
                event_authority: program.find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build pending-admin acceptance; the nominee must sign.
pub fn accept_admin_instruction(program: &EvmProxyProgram, next_admin: Pubkey) -> Instruction {
    let program_id = *program.id;
    program
        .anchor_instruction(args::AcceptAdmin {})
        .anchor_accounts(
            accounts::AcceptAdmin {
                next_admin,
                config: find_proxy_config_address(&program_id).0,
                event_authority: program.find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Config fields read after initialization confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigReadback {
    /// Immutable bound Store.
    pub store: Pubkey,
    /// Immutable supplied genesis hash.
    pub genesis_hash: [u8; 32],
    /// Admin observed at readback time; no handover history is verified.
    pub current_admin: Pubkey,
}

/// Confirmed initialization and its separate immutable-field verification.
#[derive(Debug)]
pub enum ConfigInitialization {
    /// Store and genesis hash matched; this does not verify admin handover history.
    ImmutableFieldsVerified {
        /// Confirmed creation signature.
        signature: Signature,
        /// Current admin observed at readback time.
        current_admin: Pubkey,
    },
    /// Creation was confirmed but immutable-field readback failed or mismatched.
    VerificationIncomplete {
        /// Confirmed signature to inspect before any further action.
        signature: Signature,
        /// Readback failure or mismatch.
        reason: String,
    },
}

fn verify_readback(
    signature: Signature,
    store: Pubkey,
    genesis_hash: [u8; 32],
    readback: crate::Result<ConfigReadback>,
) -> ConfigInitialization {
    let verified = readback.and_then(|account| {
        if account.store != store || account.genesis_hash != genesis_hash {
            Err(crate::Error::custom(
                "Config immutable fields differ from initialization",
            ))
        } else {
            Ok(account.current_admin)
        }
    });
    match verified {
        Ok(current_admin) => ConfigInitialization::ImmutableFieldsVerified {
            signature,
            current_admin,
        },
        Err(error) => ConfigInitialization::VerificationIncomplete {
            signature,
            reason: error.to_string(),
        },
    }
}

/// Client operations for this client's EVM proxy deployment.
pub trait EvmProxyOps<C> {
    /// Submit a signed Cancel as this client's payer.
    fn cancel_proxy_intent(
        &self,
        chain_id: ChainId,
        intent: gmsol_evm_proxy_intent::Cancel,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Change allowlist membership as this client's payer.
    fn set_proxy_chain_allowed(
        &self,
        chain_id: [u8; 32],
        allowed: bool,
    ) -> TransactionBuilder<'_, C>;

    /// Change the pause flag as this client's payer.
    fn set_proxy_paused(&self, paused: bool) -> TransactionBuilder<'_, C>;

    /// Nominate the next admin as this client's payer; nominating the payer
    /// withdraws the pending nomination.
    fn transfer_proxy_admin(&self, next_admin: Pubkey) -> TransactionBuilder<'_, C>;

    /// Accept a nomination with this client's payer as the nominee.
    fn accept_proxy_admin(&self) -> TransactionBuilder<'_, C>;

    /// Decode the singleton Config.
    fn proxy_config_account(
        &self,
    ) -> impl Future<Output = crate::Result<gmsol_programs::gmsol_evm_proxy::accounts::Config>>;

    /// Read immutable Config fields and the currently observed admin.
    fn proxy_config(&self) -> impl Future<Output = crate::Result<ConfigReadback>>;

    /// Initialize once after a same-endpoint genesis check, then verify immutable fields.
    /// A readback failure retains the confirmed signature and never resends creation.
    fn initialize_proxy_config(
        &self,
        store: Pubkey,
        genesis_hash: [u8; 32],
        first_admin: Pubkey,
        upgrade_authority: &dyn Signer,
    ) -> impl Future<Output = crate::Result<ConfigInitialization>>;
}

fn evm_proxy_transaction<C: Deref<Target = impl Signer> + Clone>(
    client: &Client<C>,
    ix: Instruction,
) -> TransactionBuilder<'_, C> {
    client
        .program(ix.program_id)
        .transaction()
        .accounts(ix.accounts)
        .args(ix.data)
}

fn create_config_transaction<'a, C: Deref<Target = impl Signer> + Clone>(
    client: &'a Client<C>,
    store: Pubkey,
    genesis_hash: [u8; 32],
    first_admin: Pubkey,
    upgrade_authority: &'a dyn Signer,
) -> TransactionBuilder<'a, C> {
    let ix = create_config_instruction(
        client.evm_proxy_program_for_builders(),
        client.payer(),
        upgrade_authority.pubkey(),
        store,
        genesis_hash,
        first_admin,
    );
    let builder = evm_proxy_transaction(client, ix);
    if upgrade_authority.pubkey() == client.payer() {
        builder
    } else {
        builder.signer(upgrade_authority)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> EvmProxyOps<C> for Client<C> {
    fn cancel_proxy_intent(
        &self,
        chain_id: ChainId,
        intent: gmsol_evm_proxy_intent::Cancel,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let ag = Cancel::builder()
            .program(self.evm_proxy_program_for_builders().clone())
            .payer(self.payer())
            .chain_id(chain_id)
            .intent(intent)
            .signature(signature)
            .recovery_id(recovery_id)
            .build()
            .into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn set_proxy_chain_allowed(
        &self,
        chain_id: [u8; 32],
        allowed: bool,
    ) -> TransactionBuilder<'_, C> {
        evm_proxy_transaction(
            self,
            set_chain_allowed_instruction(
                self.evm_proxy_program_for_builders(),
                self.payer(),
                chain_id,
                allowed,
            ),
        )
    }

    fn set_proxy_paused(&self, paused: bool) -> TransactionBuilder<'_, C> {
        evm_proxy_transaction(
            self,
            set_paused_instruction(self.evm_proxy_program_for_builders(), self.payer(), paused),
        )
    }

    fn transfer_proxy_admin(&self, next_admin: Pubkey) -> TransactionBuilder<'_, C> {
        evm_proxy_transaction(
            self,
            transfer_admin_instruction(
                self.evm_proxy_program_for_builders(),
                self.payer(),
                next_admin,
            ),
        )
    }

    fn accept_proxy_admin(&self) -> TransactionBuilder<'_, C> {
        evm_proxy_transaction(
            self,
            accept_admin_instruction(self.evm_proxy_program_for_builders(), self.payer()),
        )
    }

    async fn proxy_config_account(
        &self,
    ) -> crate::Result<gmsol_programs::gmsol_evm_proxy::accounts::Config> {
        let program_id = *self.evm_proxy_program_for_builders().id;
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

    async fn proxy_config(&self) -> crate::Result<ConfigReadback> {
        let config = self.proxy_config_account().await?;
        Ok(ConfigReadback {
            store: config.store,
            genesis_hash: config.genesis_hash,
            current_admin: config.admin,
        })
    }

    async fn initialize_proxy_config(
        &self,
        store: Pubkey,
        genesis_hash: [u8; 32],
        first_admin: Pubkey,
        upgrade_authority: &dyn Signer,
    ) -> crate::Result<ConfigInitialization> {
        let actual = self
            .rpc()
            .get_genesis_hash()
            .await
            .map_err(crate::Error::custom)?;
        if actual.to_bytes() != genesis_hash {
            return Err(crate::Error::custom(
                "supplied genesis hash differs from RPC",
            ));
        }
        let signature =
            create_config_transaction(self, store, genesis_hash, first_admin, upgrade_authority)
                .send()
                .await?;
        Ok(verify_readback(
            signature,
            store,
            genesis_hash,
            self.proxy_config().await,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::{hash::Hash, signature::Keypair};

    use super::*;
    use crate::{
        client::ClientOptions,
        pda::{find_event_authority_address, find_proxy_config_address},
    };

    const GENESIS_HASH: [u8; 32] = [5; 32];
    const OTHER_GENESIS_HASH: [u8; 32] = [7; 32];
    const CHAIN_ID: [u8; 32] = [1; 32];

    #[test]
    fn immutable_verification_reports_observed_admin_without_a_handover_claim() {
        let signature = Signature::new_unique();
        let store = Pubkey::new_unique();
        let genesis_hash = GENESIS_HASH;
        let current_admin = Pubkey::new_unique();
        let result = verify_readback(
            signature,
            store,
            genesis_hash,
            Ok(ConfigReadback {
                store,
                genesis_hash,
                current_admin,
            }),
        );
        assert!(
            matches!(result, ConfigInitialization::ImmutableFieldsVerified {
            signature: actual_signature, current_admin: actual_admin,
        } if actual_signature == signature && actual_admin == current_admin)
        );
    }

    #[test]
    fn immutable_mismatch_preserves_confirmed_signature() {
        let signature = Signature::new_unique();
        let store = Pubkey::new_unique();
        let genesis_hash = GENESIS_HASH;
        let current_admin = Pubkey::new_unique();
        for readback in [
            ConfigReadback {
                store: Pubkey::new_unique(),
                genesis_hash,
                current_admin,
            },
            ConfigReadback {
                store,
                genesis_hash: OTHER_GENESIS_HASH,
                current_admin,
            },
        ] {
            let result = verify_readback(signature, store, genesis_hash, Ok(readback));
            assert!(
                matches!(result, ConfigInitialization::VerificationIncomplete {
                signature: actual_signature, ..
            } if actual_signature == signature)
            );
        }
    }

    #[test]
    fn readback_failure_preserves_confirmed_signature() {
        let signature = Signature::new_unique();
        let result = verify_readback(
            signature,
            Pubkey::new_unique(),
            GENESIS_HASH,
            Err(crate::Error::custom("Config readback unavailable")),
        );
        assert!(
            matches!(result, ConfigInitialization::VerificationIncomplete {
            signature: actual_signature, ..
        } if actual_signature == signature)
        );
    }

    #[test]
    fn config_builders_bind_selected_program_and_cpi_event_accounts() {
        const CREATE_CONFIG_EVENT_AUTHORITY_INDEX: usize = 8;
        const POLICY_EVENT_AUTHORITY_INDEX: usize = 2;
        const TRANSFER_ADMIN_EVENT_AUTHORITY_INDEX: usize = 3;
        const CANCEL_EVENT_AUTHORITY_INDEX: usize = 3;
        const CREATE_CONFIG_PROXY_PROGRAM_INDEX: usize = 3;
        const EVENT_ACCOUNT_COUNT: usize = 2;
        let program = Pubkey::new_unique();
        let deployment = EvmProxyProgram::from(program);
        let admin = Pubkey::new_unique();
        let first_admin = Pubkey::new_unique();
        let instruction = create_config_instruction(
            &deployment,
            admin,
            admin,
            Pubkey::new_unique(),
            GENESIS_HASH,
            first_admin,
        );
        let config = find_proxy_config_address(&program).0;
        let program_data = bpf_loader_upgradeable::get_program_data_address(&program);
        assert_eq!(instruction.program_id, program);
        for address in [config, program_data, program, first_admin] {
            assert!(instruction
                .accounts
                .iter()
                .any(|meta| meta.pubkey == address));
        }
        assert_eq!(
            instruction.accounts[CREATE_CONFIG_PROXY_PROGRAM_INDEX].pubkey,
            program
        );
        let cancel = Cancel::builder()
            .program(deployment.clone())
            .payer(admin)
            .chain_id(CHAIN_ID)
            .intent(gmsol_evm_proxy_intent::Cancel {
                proxy_account: Pubkey::new_unique(),
                nonce: 0,
                deadline: 0,
            })
            .signature([0; 64])
            .recovery_id(0)
            .build()
            .into_atomic_group(&())
            .unwrap()
            .instructions_with_options(Default::default())
            .find(|instruction| instruction.program_id == program)
            .unwrap()
            .into_owned();
        let event_authority = find_event_authority_address(&program).0;
        for (instruction, event_index) in [
            (instruction, CREATE_CONFIG_EVENT_AUTHORITY_INDEX),
            (
                set_chain_allowed_instruction(&deployment, admin, CHAIN_ID, true),
                POLICY_EVENT_AUTHORITY_INDEX,
            ),
            (
                set_paused_instruction(&deployment, admin, false),
                POLICY_EVENT_AUTHORITY_INDEX,
            ),
            (
                transfer_admin_instruction(&deployment, admin, first_admin),
                TRANSFER_ADMIN_EVENT_AUTHORITY_INDEX,
            ),
            (
                accept_admin_instruction(&deployment, first_admin),
                POLICY_EVENT_AUTHORITY_INDEX,
            ),
            (cancel, CANCEL_EVENT_AUTHORITY_INDEX),
        ] {
            assert_eq!(instruction.program_id, program);
            assert_eq!(
                instruction.accounts.len(),
                event_index + EVENT_ACCOUNT_COUNT
            );
            assert_eq!(
                instruction.accounts[event_index],
                solana_sdk::instruction::AccountMeta::new_readonly(event_authority, false)
            );
            assert_eq!(
                instruction.accounts[event_index + 1],
                solana_sdk::instruction::AccountMeta::new_readonly(program, false)
            );
        }
    }

    #[test]
    fn payer_as_upgrade_authority_signs_one_slot() {
        let payer = Arc::new(Keypair::new());
        let client = Client::new(Cluster::Localnet, payer.clone()).unwrap();
        let transaction = create_config_transaction(
            &client,
            Pubkey::new_unique(),
            GENESIS_HASH,
            Pubkey::new_unique(),
            &*payer,
        )
        .signed_transaction_with_blockhash_and_options(Hash::new_unique(), true, None, None, |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(transaction.signatures.len(), 1);
    }

    #[test]
    fn distinct_upgrade_authority_and_payer_both_sign() {
        let payer = Arc::new(Keypair::new());
        let authority = Keypair::new();
        let client = Client::new(Cluster::Localnet, payer).unwrap();
        let transaction = create_config_transaction(
            &client,
            Pubkey::new_unique(),
            GENESIS_HASH,
            Pubkey::new_unique(),
            &authority,
        )
        .signed_transaction_with_blockhash_and_options(Hash::new_unique(), true, None, None, |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(transaction.signatures.len(), 2);
    }

    #[test]
    fn client_option_selects_proxy_deployment() {
        let program = Pubkey::new_unique();
        let client = Client::new_with_options(
            Cluster::Localnet,
            Arc::new(Keypair::new()),
            ClientOptions::builder()
                .evm_proxy_program_id(Some(program))
                .build(),
        )
        .unwrap();
        let instruction = client.set_proxy_paused(false).take_instruction().unwrap();
        assert_eq!(instruction.program_id, program);
    }
}
