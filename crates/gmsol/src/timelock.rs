use std::{future::Future, ops::Deref};

use anchor_client::{
    anchor_lang::{prelude::AccountMeta, system_program},
    solana_sdk::{instruction::Instruction, pubkey::Pubkey, signer::Signer},
};
use gmsol_timelock::{
    accounts, instruction, roles,
    states::{Executor, InstructionAccess, InstructionHeader},
};

use crate::utils::{RpcBuilder, ZeroCopy};

/// Timelock instructions.
pub trait TimelockOps<C> {
    /// Initialize [`TimelockConfig`](crate::types::timelock::TimelockConfig) account.
    fn initialize_timelock_config(
        &self,
        store: &Pubkey,
        initial_delay: u32,
    ) -> RpcBuilder<C, Pubkey>;

    /// Increase timelock delay.
    fn increase_timelock_delay(&self, store: &Pubkey, delta: u32) -> RpcBuilder<C>;

    /// Initialize [`Executor`](crate::types::timelock::Executor) account.
    fn initialize_executor(
        &self,
        store: &Pubkey,
        role: &str,
    ) -> crate::Result<RpcBuilder<C, Pubkey>>;

    /// Create a timelocked instruction buffer for the given instruction.
    fn create_timelocked_instruction(
        &self,
        store: &Pubkey,
        role: &str,
        buffer: impl Signer + 'static,
        instruction: Instruction,
    ) -> crate::Result<RpcBuilder<C, Pubkey>>;

    /// Approve timelocked instruction.
    fn approve_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        role_hint: Option<&str>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Cancel timelocked instruction.
    fn cancel_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        executor_hint: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Execute timelocked instruction.
    fn execute_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        hint: Option<(&Pubkey, &[AccountMeta])>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Timelock-bypassed revoke role.
    fn timelock_bypassed_revoke_role(
        &self,
        store: &Pubkey,
        role: &str,
        address: &Pubkey,
    ) -> RpcBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> TimelockOps<C> for crate::Client<C> {
    fn initialize_timelock_config(
        &self,
        store: &Pubkey,
        initial_delay: u32,
    ) -> RpcBuilder<C, Pubkey> {
        let config = self.find_timelock_config_address(store);
        self.timelock_rpc()
            .args(instruction::InitializeConfig {
                delay: initial_delay,
            })
            .accounts(accounts::InitializeConfig {
                authority: self.payer(),
                store: *store,
                timelock_config: config,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .with_output(config)
    }

    fn increase_timelock_delay(&self, store: &Pubkey, delta: u32) -> RpcBuilder<C> {
        self.timelock_rpc()
            .args(instruction::IncreaseDelay { delta })
            .accounts(accounts::IncreaseDelay {
                authority: self.payer(),
                store: *store,
                timelock_config: self.find_timelock_config_address(store),
                store_program: *self.store_program_id(),
            })
    }

    fn initialize_executor(
        &self,
        store: &Pubkey,
        role: &str,
    ) -> crate::Result<RpcBuilder<C, Pubkey>> {
        let executor = self.find_executor_address(store, role)?;
        Ok(self
            .timelock_rpc()
            .args(instruction::InitializeExecutor {
                role: role.to_string(),
            })
            .accounts(accounts::InitializeExecutor {
                payer: self.payer(),
                store: *store,
                executor,
                system_program: system_program::ID,
            })
            .with_output(executor))
    }

    fn create_timelocked_instruction(
        &self,
        store: &Pubkey,
        role: &str,
        buffer: impl Signer + 'static,
        instruction: Instruction,
    ) -> crate::Result<RpcBuilder<C, Pubkey>> {
        let executor = self.find_executor_address(store, role)?;
        let instruction_buffer = buffer.pubkey();
        let rpc = self
            .timelock_rpc()
            .args(instruction::CreateInstructionBuffer {
                num_accounts: instruction
                    .accounts
                    .len()
                    .try_into()
                    .map_err(|_| crate::Error::invalid_argument("too many accounts"))?,
                data_len: instruction
                    .data
                    .len()
                    .try_into()
                    .map_err(|_| crate::Error::invalid_argument("data too long"))?,
                data: instruction.data,
            })
            .accounts(accounts::CreateInstructionBuffer {
                authority: self.payer(),
                store: *store,
                executor,
                instruction_buffer,
                instruction_program: instruction.program_id,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .accounts(instruction.accounts)
            .owned_signer(buffer)
            .with_output(instruction_buffer);
        Ok(rpc)
    }

    async fn approve_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        role_hint: Option<&str>,
    ) -> crate::Result<RpcBuilder<C>> {
        let role = match role_hint {
            Some(role) => role.to_string(),
            None => {
                let instruction_header = self
                    .account::<ZeroCopy<InstructionHeader>>(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let executor = instruction_header.executor();
                let executor = self
                    .account::<ZeroCopy<Executor>>(executor)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                executor.role_name()?.to_string()
            }
        };
        let executor = self.find_executor_address(store, &role)?;
        Ok(self
            .timelock_rpc()
            .args(instruction::ApproveInstruction { role })
            .accounts(accounts::ApproveInstruction {
                authority: self.payer(),
                store: *store,
                executor,
                instruction: *buffer,
                store_program: *self.store_program_id(),
            }))
    }

    async fn cancel_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        executor_hint: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        let executor = match executor_hint {
            Some(address) => *address,
            None => {
                let instruction_header = self
                    .account::<ZeroCopy<InstructionHeader>>(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                *instruction_header.executor()
            }
        };
        Ok(self
            .timelock_rpc()
            .args(instruction::CancelInstruction {})
            .accounts(accounts::CancelInstruction {
                authority: self.payer(),
                store: *store,
                executor,
                instruction: *buffer,
                store_program: *self.store_program_id(),
            }))
    }

    async fn execute_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        hint: Option<(&Pubkey, &[AccountMeta])>,
    ) -> crate::Result<RpcBuilder<C>> {
        let (executor, accounts) = match hint {
            Some((executor, accounts)) => (*executor, accounts.to_owned()),
            None => {
                let buffer = self
                    .instruction_buffer(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let executor = buffer.header().executor();
                (
                    *executor,
                    buffer.accounts().map(AccountMeta::from).collect(),
                )
            }
        };
        Ok(self
            .timelock_rpc()
            .args(instruction::ExecuteInstruction {})
            .accounts(accounts::ExecuteInstruction {
                authority: self.payer(),
                store: *store,
                timelock_config: self.find_timelock_config_address(store),
                executor,
                instruction: *buffer,
                store_program: *self.store_program_id(),
            })
            .accounts(accounts))
    }

    fn timelock_bypassed_revoke_role(
        &self,
        store: &Pubkey,
        role: &str,
        address: &Pubkey,
    ) -> RpcBuilder<C> {
        let executor = self
            .find_executor_address(store, roles::ADMIN)
            .expect("must success");
        self.timelock_rpc()
            .args(instruction::RevokeRole {
                role: role.to_string(),
            })
            .accounts(accounts::RevokeRole {
                authority: self.payer(),
                store: *store,
                executor,
                user: *address,
                store_program: *self.store_program_id(),
            })
    }
}
