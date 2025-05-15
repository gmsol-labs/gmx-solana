use std::{future::Future, ops::Deref, sync::Arc};

use gmsol_programs::{
    constants::roles,
    gmsol_timelock::{
        accounts::{Executor, InstructionHeader},
        client::{accounts, args},
    },
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::{instruction::InstructionAccess, oracle::PriceProviderKind, role::RoleKey};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::Signer,
    system_program,
};

use crate::utils::zero_copy::ZeroCopy;

/// Timelock instructions.
pub trait TimelockOps<C> {
    /// Initialize [`TimelockConfig`](crate::types::timelock::TimelockConfig) account.
    fn initialize_timelock_config(
        &self,
        store: &Pubkey,
        initial_delay: u32,
    ) -> TransactionBuilder<C, Pubkey>;

    /// Increase timelock delay.
    fn increase_timelock_delay(&self, store: &Pubkey, delta: u32) -> TransactionBuilder<C>;

    /// Initialize [`Executor`] account.
    fn initialize_executor(
        &self,
        store: &Pubkey,
        role: &str,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>>;

    /// Create a timelocked instruction buffer for the given instruction.
    fn create_timelocked_instruction(
        &self,
        store: &Pubkey,
        role: &str,
        buffer: impl Signer + 'static,
        instruction: Instruction,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>>;

    /// Approve timelocked instruction.
    fn approve_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        role_hint: Option<&str>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Approve timelocked instruction.
    fn approve_timelocked_instructions(
        &self,
        store: &Pubkey,
        buffers: impl IntoIterator<Item = Pubkey>,
        role_hint: Option<&str>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Cancel timelocked instruction.
    fn cancel_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        executor_hint: Option<&Pubkey>,
        rent_receiver_hint: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Cancel timelocked instruction.
    fn cancel_timelocked_instructions(
        &self,
        store: &Pubkey,
        buffers: impl IntoIterator<Item = Pubkey>,
        executor_hint: Option<&Pubkey>,
        rent_receiver_hint: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Execute timelocked instruction.
    fn execute_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        hint: Option<ExecuteTimelockedInstructionHint<'_>>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Timelock-bypassed revoke role.
    fn timelock_bypassed_revoke_role(
        &self,
        store: &Pubkey,
        role: &str,
        address: &Pubkey,
    ) -> TransactionBuilder<C>;

    /// Timelock-bypassed set expected price provider.
    fn timelock_bypassed_set_epxected_price_provider(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        new_expected_price_provider: PriceProviderKind,
    ) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> TimelockOps<C> for crate::Client<C> {
    fn initialize_timelock_config(
        &self,
        store: &Pubkey,
        initial_delay: u32,
    ) -> TransactionBuilder<C, Pubkey> {
        let config = self.find_timelock_config_address(store);
        let executor = self
            .find_executor_address(store, roles::ADMIN)
            .expect("must success");
        self.timelock_transaction()
            .anchor_args(args::InitializeConfig {
                delay: initial_delay,
            })
            .anchor_accounts(accounts::InitializeConfig {
                authority: self.payer(),
                store: *store,
                timelock_config: config,
                executor,
                wallet: self.find_executor_wallet_address(&executor),
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .output(config)
    }

    fn increase_timelock_delay(&self, store: &Pubkey, delta: u32) -> TransactionBuilder<C> {
        self.timelock_transaction()
            .anchor_args(args::IncreaseDelay { delta })
            .anchor_accounts(accounts::IncreaseDelay {
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
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let executor = self.find_executor_address(store, role)?;
        let wallet = self.find_executor_wallet_address(&executor);
        Ok(self
            .timelock_transaction()
            .anchor_args(args::InitializeExecutor {
                role: role.to_string(),
            })
            .anchor_accounts(accounts::InitializeExecutor {
                payer: self.payer(),
                store: *store,
                executor,
                wallet,
                system_program: system_program::ID,
            })
            .output(executor))
    }

    fn create_timelocked_instruction(
        &self,
        store: &Pubkey,
        role: &str,
        buffer: impl Signer + 'static,
        mut instruction: Instruction,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let executor = self.find_executor_address(store, role)?;
        let instruction_buffer = buffer.pubkey();

        let mut signers = vec![];

        instruction
            .accounts
            .iter_mut()
            .enumerate()
            .for_each(|(idx, account)| {
                if account.is_signer {
                    signers.push(idx as u16);
                }
                account.is_signer = false;
            });

        let num_accounts = instruction
            .accounts
            .len()
            .try_into()
            .map_err(|_| crate::Error::unknown("too many accounts"))?;

        let data_len = instruction
            .data
            .len()
            .try_into()
            .map_err(|_| crate::Error::unknown("data too long"))?;

        let rpc = self
            .timelock_transaction()
            .anchor_args(args::CreateInstructionBuffer {
                num_accounts,
                data_len,
                data: instruction.data,
                signers,
            })
            .anchor_accounts(accounts::CreateInstructionBuffer {
                authority: self.payer(),
                store: *store,
                executor,
                instruction_buffer,
                instruction_program: instruction.program_id,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .accounts(instruction.accounts)
            .owned_signer(Arc::new(buffer))
            .output(instruction_buffer);
        Ok(rpc)
    }

    async fn approve_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        role_hint: Option<&str>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let role = match role_hint {
            Some(role) => role.to_string(),
            None => {
                let instruction_header = self
                    .account::<ZeroCopy<InstructionHeader>>(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let executor = &instruction_header.executor;
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
            .timelock_transaction()
            .anchor_args(args::ApproveInstruction { role })
            .anchor_accounts(accounts::ApproveInstruction {
                authority: self.payer(),
                store: *store,
                executor,
                instruction: *buffer,
                store_program: *self.store_program_id(),
            }))
    }

    async fn approve_timelocked_instructions(
        &self,
        store: &Pubkey,
        buffers: impl IntoIterator<Item = Pubkey>,
        role_hint: Option<&str>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let mut buffers = buffers.into_iter().peekable();
        let buffer = buffers
            .peek()
            .ok_or_else(|| crate::Error::unknown("no instructions to appove"))?;
        let role = match role_hint {
            Some(role) => role.to_string(),
            None => {
                let instruction_header = self
                    .account::<ZeroCopy<InstructionHeader>>(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let executor = &instruction_header.executor;
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
            .timelock_transaction()
            .anchor_args(args::ApproveInstructions { role })
            .anchor_accounts(accounts::ApproveInstructions {
                authority: self.payer(),
                store: *store,
                executor,
                store_program: *self.store_program_id(),
            })
            .accounts(
                buffers
                    .map(|pubkey| AccountMeta {
                        pubkey,
                        is_signer: false,
                        is_writable: true,
                    })
                    .collect::<Vec<_>>(),
            ))
    }

    async fn cancel_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        executor_hint: Option<&Pubkey>,
        rent_receiver_hint: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (executor, rent_receiver) = match (executor_hint, rent_receiver_hint) {
            (Some(executor), Some(rent_receiver)) => (*executor, *rent_receiver),
            _ => {
                let instruction_header = self
                    .account::<ZeroCopy<InstructionHeader>>(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                (
                    instruction_header.executor,
                    instruction_header.rent_receiver,
                )
            }
        };
        Ok(self
            .timelock_transaction()
            .anchor_args(args::CancelInstruction {})
            .anchor_accounts(accounts::CancelInstruction {
                authority: self.payer(),
                store: *store,
                executor,
                rent_receiver,
                instruction: *buffer,
                store_program: *self.store_program_id(),
            }))
    }

    async fn cancel_timelocked_instructions(
        &self,
        store: &Pubkey,
        buffers: impl IntoIterator<Item = Pubkey>,
        executor_hint: Option<&Pubkey>,
        rent_receiver_hint: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let mut buffers = buffers.into_iter().peekable();
        let buffer = buffers
            .peek()
            .ok_or_else(|| crate::Error::unknown("no instructions to appove"))?;
        let (executor, rent_receiver) = match (executor_hint, rent_receiver_hint) {
            (Some(executor), Some(rent_receiver)) => (*executor, *rent_receiver),
            _ => {
                let instruction_header = self
                    .account::<ZeroCopy<InstructionHeader>>(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                (
                    instruction_header.executor,
                    instruction_header.rent_receiver,
                )
            }
        };
        Ok(self
            .timelock_transaction()
            .anchor_args(args::CancelInstructions {})
            .anchor_accounts(accounts::CancelInstructions {
                authority: self.payer(),
                store: *store,
                executor,
                rent_receiver,
                store_program: *self.store_program_id(),
            })
            .accounts(
                buffers
                    .map(|pubkey| AccountMeta {
                        pubkey,
                        is_signer: false,
                        is_writable: true,
                    })
                    .collect::<Vec<_>>(),
            ))
    }

    async fn execute_timelocked_instruction(
        &self,
        store: &Pubkey,
        buffer: &Pubkey,
        hint: Option<ExecuteTimelockedInstructionHint<'_>>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (executor, rent_receiver, mut accounts) = match hint {
            Some(hint) => (
                *hint.executor,
                *hint.rent_receiver,
                hint.accounts.to_owned(),
            ),
            None => {
                let buffer = self
                    .instruction_buffer(buffer)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let executor = buffer.header.executor;
                let rent_receiver = buffer.header.rent_receiver;
                (
                    executor,
                    rent_receiver,
                    buffer.accounts().map(AccountMeta::from).collect(),
                )
            }
        };

        let wallet = self.find_executor_wallet_address(&executor);

        accounts
            .iter_mut()
            .filter(|a| a.pubkey == wallet)
            .for_each(|a| a.is_signer = false);

        Ok(self
            .timelock_transaction()
            .anchor_args(args::ExecuteInstruction {})
            .anchor_accounts(accounts::ExecuteInstruction {
                authority: self.payer(),
                store: *store,
                timelock_config: self.find_timelock_config_address(store),
                executor,
                wallet,
                rent_receiver,
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
    ) -> TransactionBuilder<C> {
        let executor = self
            .find_executor_address(store, roles::ADMIN)
            .expect("must success");
        let wallet = self.find_executor_wallet_address(&executor);
        self.timelock_transaction()
            .anchor_args(args::RevokeRole {
                role: role.to_string(),
            })
            .anchor_accounts(accounts::RevokeRole {
                authority: self.payer(),
                store: *store,
                executor,
                wallet,
                user: *address,
                store_program: *self.store_program_id(),
            })
    }

    fn timelock_bypassed_set_epxected_price_provider(
        &self,
        store: &Pubkey,
        token_map: &Pubkey,
        token: &Pubkey,
        new_expected_price_provider: PriceProviderKind,
    ) -> TransactionBuilder<C> {
        let executor = self
            .find_executor_address(store, RoleKey::MARKET_KEEPER)
            .expect("must success");
        let wallet = self.find_executor_wallet_address(&executor);
        self.timelock_transaction()
            .anchor_args(args::SetExpectedPriceProvider {
                new_expected_price_provider: new_expected_price_provider.into(),
            })
            .anchor_accounts(accounts::SetExpectedPriceProvider {
                authority: self.payer(),
                store: *store,
                executor,
                wallet,
                token_map: *token_map,
                token: *token,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
    }
}

/// Execute timelocked instruction hint.
#[derive(Debug)]
pub struct ExecuteTimelockedInstructionHint<'a> {
    /// Executor.
    pub executor: &'a Pubkey,
    /// Rent receiver.
    pub rent_receiver: &'a Pubkey,
    /// Accounts.
    pub accounts: &'a [AccountMeta],
}
