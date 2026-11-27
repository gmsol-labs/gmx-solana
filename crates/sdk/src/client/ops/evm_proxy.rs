//! EVM proxy Config operations and immutable initialization verification.

use std::{future::Future, ops::Deref};

use gmsol_programs::{
    anchor_lang::AccountDeserialize,
    gmsol_evm_proxy::client::{accounts, args},
};
use gmsol_solana_utils::{transaction_builder::TransactionBuilder, ProgramExt};
use solana_sdk::{
    bpf_loader_upgradeable, instruction::Instruction, pubkey::Pubkey, signature::Signature,
    signer::Signer, system_program,
};

use crate::{builders::evm_proxy::EvmProxyProgram, pda::find_proxy_config_address, Client};

/// Build the one-time Config initialization instruction.
pub fn create_config_instruction(
    program_id: Pubkey,
    payer: Pubkey,
    upgrade_authority: Pubkey,
    store: Pubkey,
    genesis_hash: [u8; 32],
    first_admin: Pubkey,
) -> Instruction {
    EvmProxyProgram::from(program_id)
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
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build one current-admin chain allowlist mutation.
pub fn set_chain_allowed_instruction(
    program_id: Pubkey,
    admin: Pubkey,
    chain_id: [u8; 32],
    allowed: bool,
) -> Instruction {
    EvmProxyProgram::from(program_id)
        .anchor_instruction(args::SetChainAllowed { chain_id, allowed })
        .anchor_accounts(
            accounts::SetChainAllowed {
                admin,
                config: find_proxy_config_address(&program_id).0,
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build one current-admin pause mutation.
pub fn set_paused_instruction(program_id: Pubkey, admin: Pubkey, paused: bool) -> Instruction {
    EvmProxyProgram::from(program_id)
        .anchor_instruction(args::SetPaused { paused })
        .anchor_accounts(
            accounts::SetPaused {
                admin,
                config: find_proxy_config_address(&program_id).0,
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build a current-admin handover proposal or replacement.
pub fn transfer_admin_instruction(
    program_id: Pubkey,
    admin: Pubkey,
    next_admin: Pubkey,
) -> Instruction {
    EvmProxyProgram::from(program_id)
        .anchor_instruction(args::TransferAdmin {})
        .anchor_accounts(
            accounts::TransferAdmin {
                admin,
                config: find_proxy_config_address(&program_id).0,
                next_admin,
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build()
}

/// Build pending-admin acceptance; the nominee must sign.
pub fn accept_admin_instruction(program_id: Pubkey, next_admin: Pubkey) -> Instruction {
    EvmProxyProgram::from(program_id)
        .anchor_instruction(args::AcceptAdmin {})
        .anchor_accounts(
            accounts::AcceptAdmin {
                next_admin,
                config: find_proxy_config_address(&program_id).0,
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
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

/// Client operations for an explicitly selected EVM proxy deployment.
pub trait EvmProxyOps<C> {
    /// Submit a signed Cancel using this client's transaction options.
    fn cancel_proxy_intent(
        &self,
        program_id: Pubkey,
        intent: &crate::evm_proxy::CancelIntent,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Wrap an instruction using its program ID and this client's transaction options.
    fn evm_proxy_instruction(&self, ix: Instruction) -> TransactionBuilder<'_, C>;

    /// Change allowlist membership as this client's payer.
    fn set_proxy_chain_allowed(
        &self,
        program_id: Pubkey,
        chain_id: [u8; 32],
        allowed: bool,
    ) -> TransactionBuilder<'_, C>;

    /// Change the pause flag as this client's payer.
    fn set_proxy_paused(&self, program_id: Pubkey, paused: bool) -> TransactionBuilder<'_, C>;

    /// Nominate the next admin as this client's payer.
    fn transfer_proxy_admin(
        &self,
        program_id: Pubkey,
        next_admin: Pubkey,
    ) -> TransactionBuilder<'_, C>;

    /// Accept a nomination with this client's payer as the nominee.
    fn accept_proxy_admin(&self, program_id: Pubkey) -> TransactionBuilder<'_, C>;

    /// Decode the selected deployment's singleton Config.
    fn proxy_config_account(
        &self,
        program_id: Pubkey,
    ) -> impl Future<Output = crate::Result<gmsol_programs::gmsol_evm_proxy::accounts::Config>>;

    /// Read immutable Config fields and the currently observed admin.
    fn proxy_config(
        &self,
        program_id: Pubkey,
    ) -> impl Future<Output = crate::Result<ConfigReadback>>;

    /// Initialize once after a same-endpoint genesis check, then verify immutable fields.
    /// A readback failure retains the confirmed signature and never resends creation.
    fn initialize_proxy_config(
        &self,
        program_id: Pubkey,
        store: Pubkey,
        genesis_hash: [u8; 32],
        first_admin: Pubkey,
        upgrade_authority: &dyn Signer,
    ) -> impl Future<Output = crate::Result<ConfigInitialization>>;
}

fn create_config_transaction<'a, C: Deref<Target = impl Signer> + Clone>(
    client: &Client<C>,
    program_id: Pubkey,
    store: Pubkey,
    genesis_hash: [u8; 32],
    first_admin: Pubkey,
    upgrade_authority: &'a dyn Signer,
) -> TransactionBuilder<'a, C> {
    let ix = create_config_instruction(
        program_id,
        client.payer(),
        upgrade_authority.pubkey(),
        store,
        genesis_hash,
        first_admin,
    );
    let builder = client
        .program(program_id)
        .transaction()
        .accounts(ix.accounts)
        .args(ix.data);
    if upgrade_authority.pubkey() == client.payer() {
        builder
    } else {
        builder.signer(upgrade_authority)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> EvmProxyOps<C> for Client<C> {
    /// Submit a signed Cancel using this client's transaction options.
    fn cancel_proxy_intent(
        &self,
        program_id: Pubkey,
        intent: &crate::evm_proxy::CancelIntent,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let ix = crate::evm_proxy::cancel_instruction(
            program_id,
            self.payer(),
            intent,
            signature,
            recovery_id,
        )?;
        Ok(self.evm_proxy_instruction(ix))
    }

    fn evm_proxy_instruction(&self, ix: Instruction) -> TransactionBuilder<'_, C> {
        self.program(ix.program_id)
            .transaction()
            .accounts(ix.accounts)
            .args(ix.data)
    }

    fn set_proxy_chain_allowed(
        &self,
        program_id: Pubkey,
        chain_id: [u8; 32],
        allowed: bool,
    ) -> TransactionBuilder<'_, C> {
        self.evm_proxy_instruction(set_chain_allowed_instruction(
            program_id,
            self.payer(),
            chain_id,
            allowed,
        ))
    }

    fn set_proxy_paused(&self, program_id: Pubkey, paused: bool) -> TransactionBuilder<'_, C> {
        self.evm_proxy_instruction(set_paused_instruction(program_id, self.payer(), paused))
    }

    fn transfer_proxy_admin(
        &self,
        program_id: Pubkey,
        next_admin: Pubkey,
    ) -> TransactionBuilder<'_, C> {
        self.evm_proxy_instruction(transfer_admin_instruction(
            program_id,
            self.payer(),
            next_admin,
        ))
    }

    fn accept_proxy_admin(&self, program_id: Pubkey) -> TransactionBuilder<'_, C> {
        self.evm_proxy_instruction(accept_admin_instruction(program_id, self.payer()))
    }

    async fn proxy_config_account(
        &self,
        program_id: Pubkey,
    ) -> crate::Result<gmsol_programs::gmsol_evm_proxy::accounts::Config> {
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

    async fn proxy_config(&self, program_id: Pubkey) -> crate::Result<ConfigReadback> {
        let config = self.proxy_config_account(program_id).await?;
        Ok(ConfigReadback {
            store: config.store,
            genesis_hash: config.genesis_hash,
            current_admin: config.admin,
        })
    }

    async fn initialize_proxy_config(
        &self,
        program_id: Pubkey,
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
        let signature = create_config_transaction(
            self,
            program_id,
            store,
            genesis_hash,
            first_admin,
            upgrade_authority,
        )
        .send()
        .await?;
        Ok(verify_readback(
            signature,
            store,
            genesis_hash,
            self.proxy_config(program_id).await,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::{hash::Hash, signature::Keypair};

    use super::*;

    #[test]
    fn immutable_verification_reports_observed_admin_without_a_handover_claim() {
        let signature = Signature::new_unique();
        let store = Pubkey::new_unique();
        let genesis_hash = [5; 32];
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
        let genesis_hash = [5; 32];
        let current_admin = Pubkey::new_unique();
        for readback in [
            ConfigReadback {
                store: Pubkey::new_unique(),
                genesis_hash,
                current_admin,
            },
            ConfigReadback {
                store,
                genesis_hash: [7; 32],
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
            [5; 32],
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
        const CREATE_CONFIG_PROXY_PROGRAM_INDEX: usize = 3;
        const EVENT_ACCOUNT_COUNT: usize = 2;
        let program = Pubkey::new_unique();
        let admin = Pubkey::new_unique();
        let first_admin = Pubkey::new_unique();
        let instruction = create_config_instruction(
            program,
            admin,
            admin,
            Pubkey::new_unique(),
            [5; 32],
            first_admin,
        );
        let config = Pubkey::find_program_address(&[b"config"], &program).0;
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
        let event_authority = Pubkey::find_program_address(&[b"__event_authority"], &program).0;
        for (instruction, event_index) in [
            (instruction, CREATE_CONFIG_EVENT_AUTHORITY_INDEX),
            (
                set_chain_allowed_instruction(program, admin, [1; 32], true),
                POLICY_EVENT_AUTHORITY_INDEX,
            ),
            (
                set_paused_instruction(program, admin, false),
                POLICY_EVENT_AUTHORITY_INDEX,
            ),
            (
                transfer_admin_instruction(program, admin, first_admin),
                TRANSFER_ADMIN_EVENT_AUTHORITY_INDEX,
            ),
            (
                accept_admin_instruction(program, first_admin),
                POLICY_EVENT_AUTHORITY_INDEX,
            ),
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
            gmsol_programs::gmsol_evm_proxy::ID,
            Pubkey::new_unique(),
            [5; 32],
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
            gmsol_programs::gmsol_evm_proxy::ID,
            Pubkey::new_unique(),
            [5; 32],
            Pubkey::new_unique(),
            &authority,
        )
        .signed_transaction_with_blockhash_and_options(Hash::new_unique(), true, None, None, |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(transaction.signatures.len(), 2);
    }
}
