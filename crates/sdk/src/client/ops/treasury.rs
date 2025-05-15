use std::{future::Future, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_treasury::{
    accounts::{Config, GtBank, GtExchange, TreasuryVaultConfig},
    client::{accounts, args},
    ID,
};
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    oracle::PriceProviderKind,
    pubkey::optional_address,
    token_config::{TokenFlag, TokensWithFeed},
};
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer, system_program};

use crate::{
    builders::utils::generate_nonce,
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        program_ids,
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
    },
    pda::NonceBytes,
    utils::{optional::fix_optional_account_metas, zero_copy::ZeroCopy},
};

use super::{gt::GtOps, token_account::TokenAccountOps};

/// Operations for treasury.
pub trait TreasuryOps<C> {
    /// Initialize [`Config`] account.
    fn initialize_config(&self, store: &Pubkey) -> TransactionBuilder<C, Pubkey>;

    /// Set treasury vault config.
    fn set_treasury_vault_config(
        &self,
        store: &Pubkey,
        treasury_vault_config: &Pubkey,
    ) -> TransactionBuilder<C>;

    /// Set GT factor.
    fn set_gt_factor(&self, store: &Pubkey, factor: u128) -> crate::Result<TransactionBuilder<C>>;

    /// Set buyback factor.
    fn set_buyback_factor(
        &self,
        store: &Pubkey,
        factor: u128,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Initialize [`TreasuryVaultConfig`].
    fn initialize_treasury_vault_config(
        &self,
        store: &Pubkey,
        index: u16,
    ) -> TransactionBuilder<C, Pubkey>;

    /// Insert token to treasury.
    fn insert_token_to_treasury(
        &self,
        store: &Pubkey,
        treasury_vault_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Remove token from treasury.
    fn remove_token_from_treasury(
        &self,
        store: &Pubkey,
        treasury_vault_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Toggle token flag.
    fn toggle_token_flag(
        &self,
        store: &Pubkey,
        treasury_vault_config: Option<&Pubkey>,
        token_mint: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Deposit to treasury vault.
    fn deposit_to_treasury_valut(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        time_window: u32,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C, Pubkey>>>;

    /// Withdraw from treasury vault.
    #[allow(clippy::too_many_arguments)]
    fn withdraw_from_treasury_vault(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        amount: u64,
        decimals: u8,
        target: &Pubkey,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Confirm GT buyback.
    fn confirm_gt_buyback(
        &self,
        store: &Pubkey,
        gt_exchange_vault: &Pubkey,
        oracle: &Pubkey,
    ) -> ConfirmGtBuybackBuilder<C>;

    /// Transfer receiver.
    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> TransactionBuilder<C>;

    /// Set referral reward factors.
    fn set_referral_reward(&self, store: &Pubkey, factors: Vec<u128>) -> TransactionBuilder<C>;

    /// Claim fees to receiver vault.
    fn claim_fees_to_receiver_vault(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        token_mint: &Pubkey,
        min_amount: u64,
    ) -> TransactionBuilder<C>;

    /// Prepare GT bank.
    fn prepare_gt_bank(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C, Pubkey>>>;

    /// Sync GT bank.
    fn sync_gt_bank(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Complete GT exchange.
    fn complete_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        tokens_hint: Option<Vec<(Pubkey, Pubkey)>>,
        gt_exchange_vault_hint: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Create a swap.
    fn create_treasury_swap(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        swap_in_token: &Pubkey,
        swap_out_token: &Pubkey,
        swap_in_token_amount: u64,
        options: CreateTreasurySwapOptions,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C, Pubkey>>>;

    /// Cancel a swap.
    fn cancel_treasury_swap(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<(&Pubkey, &Pubkey)>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;
}

impl<C: Deref<Target = impl Signer> + Clone> TreasuryOps<C> for crate::Client<C> {
    fn initialize_config(&self, store: &Pubkey) -> TransactionBuilder<C, Pubkey> {
        let config = self.find_treasury_config_address(store);
        self.treasury_transaction()
            .anchor_args(args::InitializeConfig {})
            .anchor_accounts(accounts::InitializeConfig {
                payer: self.payer(),
                store: *store,
                config,
                receiver: self.find_treasury_receiver_address(&config),
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .output(config)
    }

    fn set_treasury_vault_config(
        &self,
        store: &Pubkey,
        treasury_vault_config: &Pubkey,
    ) -> TransactionBuilder<C> {
        let config = self.find_treasury_config_address(store);
        self.treasury_transaction()
            .anchor_args(args::SetTreasuryVaultConfig {})
            .anchor_accounts(accounts::SetTreasuryVaultConfig {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config: *treasury_vault_config,
                store_program: *self.store_program_id(),
            })
    }

    fn set_gt_factor(&self, store: &Pubkey, factor: u128) -> crate::Result<TransactionBuilder<C>> {
        if factor > crate::constants::MARKET_USD_UNIT {
            return Err(crate::Error::custom("cannot use a factor greater than 1"));
        }
        let config = self.find_treasury_config_address(store);
        Ok(self
            .treasury_transaction()
            .anchor_args(args::SetGtFactor { factor })
            .anchor_accounts(accounts::SetGtFactor {
                authority: self.payer(),
                store: *store,
                config,
                store_program: *self.store_program_id(),
            }))
    }

    fn set_buyback_factor(
        &self,
        store: &Pubkey,
        factor: u128,
    ) -> crate::Result<TransactionBuilder<C>> {
        if factor > crate::constants::MARKET_USD_UNIT {
            return Err(crate::Error::custom("cannot use a factor greater than 1"));
        }
        let config = self.find_treasury_config_address(store);
        Ok(self
            .treasury_transaction()
            .anchor_args(args::SetBuybackFactor { factor })
            .anchor_accounts(accounts::SetBuybackFactor {
                authority: self.payer(),
                store: *store,
                config,
                store_program: *self.store_program_id(),
            }))
    }

    fn initialize_treasury_vault_config(
        &self,
        store: &Pubkey,
        index: u16,
    ) -> TransactionBuilder<C, Pubkey> {
        let config = self.find_treasury_config_address(store);
        let treasury_vault_config = self.find_treasury_vault_config_address(&config, index);
        self.treasury_transaction()
            .anchor_args(args::InitializeTreasuryVaultConfig { index })
            .anchor_accounts(accounts::InitializeTreasuryVaultConfig {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .output(treasury_vault_config)
    }

    async fn insert_token_to_treasury(
        &self,
        store: &Pubkey,
        treasury_vault_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config).await?;
        Ok(self
            .treasury_transaction()
            .anchor_args(args::InsertTokenToTreasuryVault {})
            .anchor_accounts(accounts::InsertTokenToTreasuryVault {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                token: *token_mint,
                store_program: *self.store_program_id(),
            }))
    }

    async fn remove_token_from_treasury(
        &self,
        store: &Pubkey,
        treasury_vault_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config).await?;
        Ok(self
            .treasury_transaction()
            .anchor_args(args::RemoveTokenFromTreasuryVault {})
            .anchor_accounts(accounts::RemoveTokenFromTreasuryVault {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                token: *token_mint,
                store_program: *self.store_program_id(),
            }))
    }

    async fn toggle_token_flag(
        &self,
        store: &Pubkey,
        treasury_vault_config: Option<&Pubkey>,
        token_mint: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config).await?;
        Ok(self
            .treasury_transaction()
            .anchor_args(args::ToggleTokenFlag {
                flag: flag.to_string(),
                value,
            })
            .anchor_accounts(accounts::ToggleTokenFlag {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                token: *token_mint,
                store_program: *self.store_program_id(),
            }))
    }

    async fn deposit_to_treasury_valut(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        time_window: u32,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config_hint).await?;

        let (prepare_gt_exchange_vault, gt_exchange_vault) = self
            .prepare_gt_exchange_vault_with_time_window(store, time_window)?
            .swap_output(());

        let (prepare_gt_bank, gt_bank) = self
            .prepare_gt_bank(store, Some(&treasury_vault_config), &gt_exchange_vault)
            .await?
            .swap_output(());

        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);

        let receiver = self.find_treasury_receiver_address(&config);

        let receiver_vault =
            get_associated_token_address_with_program_id(&receiver, token_mint, token_program_id);
        let treasury_vault = get_associated_token_address_with_program_id(
            &treasury_vault_config,
            token_mint,
            token_program_id,
        );
        let gt_bank_vault =
            get_associated_token_address_with_program_id(&gt_bank, token_mint, token_program_id);

        let prepare_treasury_vault = self.prepare_associated_token_account(
            token_mint,
            token_program_id,
            Some(&treasury_vault_config),
        );
        let prepare_gt_bank_vault =
            self.prepare_associated_token_account(token_mint, token_program_id, Some(&gt_bank));

        let deposit = self
            .treasury_transaction()
            .anchor_args(args::DepositToTreasuryVault {})
            .anchor_accounts(accounts::DepositToTreasuryVault {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                receiver,
                gt_exchange_vault,
                gt_bank,
                token: *token_mint,
                receiver_vault,
                treasury_vault,
                gt_bank_vault,
                store_program: *self.store_program_id(),
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            });
        Ok(prepare_gt_exchange_vault
            .merge(prepare_gt_bank)
            .merge(prepare_treasury_vault)
            .merge(prepare_gt_bank_vault)
            .merge(deposit)
            .output(gt_exchange_vault))
    }

    async fn withdraw_from_treasury_vault(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        amount: u64,
        decimals: u8,
        target: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);

        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config_hint).await?;

        let treasury_vault = get_associated_token_address_with_program_id(
            &treasury_vault_config,
            token_mint,
            token_program_id,
        );

        Ok(self
            .treasury_transaction()
            .anchor_args(args::WithdrawFromTreasuryVault { amount, decimals })
            .anchor_accounts(accounts::WithdrawFromTreasuryVault {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                token: *token_mint,
                treasury_vault,
                target: *target,
                store_program: *self.store_program_id(),
                token_program: *token_program_id,
            }))
    }

    fn confirm_gt_buyback(
        &self,
        store: &Pubkey,
        gt_exchange_vault: &Pubkey,
        oracle: &Pubkey,
    ) -> ConfirmGtBuybackBuilder<C> {
        ConfirmGtBuybackBuilder::new(self, store, gt_exchange_vault, oracle)
    }

    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> TransactionBuilder<C> {
        let config = self.find_treasury_config_address(store);
        let receiver = self.find_treasury_receiver_address(&config);
        self.treasury_transaction()
            .anchor_args(args::TransferReceiver {})
            .anchor_accounts(accounts::TransferReceiver {
                authority: self.payer(),
                store: *store,
                config,
                receiver,
                next_receiver: *new_receiver,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
    }

    fn set_referral_reward(&self, store: &Pubkey, factors: Vec<u128>) -> TransactionBuilder<C> {
        self.treasury_transaction()
            .anchor_args(args::SetReferralReward { factors })
            .anchor_accounts(accounts::SetReferralReward {
                authority: self.payer(),
                store: *store,
                config: self.find_treasury_config_address(store),
                store_program: *self.store_program_id(),
            })
    }

    fn claim_fees_to_receiver_vault(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        token_mint: &Pubkey,
        min_amount: u64,
    ) -> TransactionBuilder<C> {
        let config = self.find_treasury_config_address(store);
        let token_program_id = anchor_spl::token::ID;
        let receiver = self.find_treasury_receiver_address(&config);
        let receiver_vault =
            get_associated_token_address_with_program_id(&receiver, token_mint, &token_program_id);
        self.treasury_transaction()
            .anchor_args(args::ClaimFees { min_amount })
            .anchor_accounts(accounts::ClaimFees {
                authority: self.payer(),
                store: *store,
                config,
                receiver,
                market: self.find_market_address(store, market_token),
                token: *token_mint,
                vault: self.find_market_vault_address(store, token_mint),
                receiver_vault,
                event_authority: self.store_event_authority(),
                store_program: *self.store_program_id(),
                token_program: token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            })
    }

    async fn prepare_gt_bank(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config_hint).await?;
        let gt_bank = self.find_gt_bank_address(&treasury_vault_config, gt_exchange_vault);
        Ok(self
            .treasury_transaction()
            .anchor_args(args::PrepareGtBank {})
            .anchor_accounts(accounts::PrepareGtBank {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                gt_exchange_vault: *gt_exchange_vault,
                gt_bank,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .output(gt_bank))
    }

    async fn sync_gt_bank(
        &self,
        store: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config_hint).await?;
        let gt_bank = self.find_gt_bank_address(&treasury_vault_config, gt_exchange_vault);
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);

        let treasury_vault = get_associated_token_address_with_program_id(
            &treasury_vault_config,
            token_mint,
            token_program_id,
        );
        let gt_bank_vault =
            get_associated_token_address_with_program_id(&gt_bank, token_mint, token_program_id);

        let prepare_treasury_vault = self.prepare_associated_token_account(
            token_mint,
            token_program_id,
            Some(&treasury_vault_config),
        );
        let prepare_gt_bank_vault =
            self.prepare_associated_token_account(token_mint, token_program_id, Some(&gt_bank));

        let sync = self
            .treasury_transaction()
            .anchor_args(args::SyncGtBank {})
            .anchor_accounts(accounts::SyncGtBank {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                gt_bank,
                token: *token_mint,
                treasury_vault,
                gt_bank_vault,
                store_program: *self.store_program_id(),
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            });

        Ok(prepare_treasury_vault
            .merge(prepare_gt_bank_vault)
            .merge(sync))
    }

    async fn complete_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        treasury_vault_config_hint: Option<&Pubkey>,
        tokens_hint: Option<Vec<(Pubkey, Pubkey)>>,
        gt_exchange_vault_hint: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, treasury_vault_config_hint).await?;
        let gt_exchange_vault = match gt_exchange_vault_hint {
            Some(address) => *address,
            None => {
                let exchange = self
                    .account::<ZeroCopy<GtExchange>>(exchange)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                exchange.vault
            }
        };
        let gt_bank = self.find_gt_bank_address(&treasury_vault_config, &gt_exchange_vault);

        let tokens = match tokens_hint {
            Some(tokens) => tokens,
            None => {
                let gt_bank = self
                    .account::<ZeroCopy<GtBank>>(&gt_bank)
                    .await?
                    .ok_or_else(|| crate::Error::custom("treasury vault config not exist"))?
                    .0;

                let tokens = gt_bank.tokens().collect::<Vec<_>>();
                self.treasury_program()
                    .rpc()
                    .get_multiple_accounts_with_config(
                        &tokens,
                        RpcAccountInfoConfig {
                            encoding: Some(UiAccountEncoding::Base64),
                            data_slice: Some(solana_account_decoder::UiDataSliceConfig {
                                offset: 0,
                                length: 0,
                            }),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(crate::Error::custom)?
                    .value
                    .into_iter()
                    .zip(&tokens)
                    .map(|(account, address)| {
                        let account = account.ok_or(crate::Error::NotFound)?;
                        Ok((*address, account.owner))
                    })
                    .collect::<crate::Result<Vec<_>>>()?
            }
        };

        let token_mints = tokens.iter().map(|pubkey| AccountMeta {
            pubkey: pubkey.0,
            is_signer: false,
            is_writable: false,
        });
        let gt_bank_vaults = tokens.iter().map(|(mint, token_program_id)| {
            let gt_bank_vault =
                get_associated_token_address_with_program_id(&gt_bank, mint, token_program_id);
            AccountMeta {
                pubkey: gt_bank_vault,
                is_signer: false,
                is_writable: true,
            }
        });
        let atas = tokens.iter().map(|(mint, token_program_id)| {
            let ata = get_associated_token_address_with_program_id(&owner, mint, token_program_id);
            AccountMeta {
                pubkey: ata,
                is_signer: false,
                is_writable: true,
            }
        });

        Ok(self
            .treasury_transaction()
            .anchor_args(args::CompleteGtExchange {})
            .anchor_accounts(accounts::CompleteGtExchange {
                owner,
                store: *store,
                config,
                treasury_vault_config,
                gt_exchange_vault,
                gt_bank,
                exchange: *exchange,
                store_program: *self.store_program_id(),
                token_program: anchor_spl::token::ID,
                token_2022_program: anchor_spl::token_2022::ID,
            })
            .accounts(
                token_mints
                    .chain(gt_bank_vaults)
                    .chain(atas)
                    .collect::<Vec<_>>(),
            ))
    }

    async fn create_treasury_swap(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        swap_in_token: &Pubkey,
        swap_out_token: &Pubkey,
        swap_in_token_amount: u64,
        options: CreateTreasurySwapOptions,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let nonce = options.nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let swap_path = options
            .swap_path
            .iter()
            .chain(Some(market_token))
            .map(|token| {
                let pubkey = self.find_market_address(store, token);
                AccountMeta {
                    pubkey,
                    is_signer: false,
                    is_writable: false,
                }
            })
            .collect::<Vec<_>>();
        let (config, treasury_vault_config) =
            find_config_addresses(self, store, options.treasury_vault_config_hint.as_ref()).await?;

        let receiver = self.find_treasury_receiver_address(&config);

        // Currently only SPL-Token is supported.
        let token_program_id = anchor_spl::token::ID;

        let swap_in_token_receiver_vault = get_associated_token_address_with_program_id(
            &receiver,
            swap_in_token,
            &token_program_id,
        );

        let market = self.find_market_address(store, market_token);

        let user = self.find_user_address(store, &receiver);

        let order = self.find_order_address(store, &receiver, &nonce);

        let swap_in_token_escrow =
            get_associated_token_address_with_program_id(&order, swap_in_token, &token_program_id);
        let swap_out_token_escrow =
            get_associated_token_address_with_program_id(&order, swap_out_token, &token_program_id);

        let prepare_swap_in_escrow =
            self.prepare_associated_token_account(swap_in_token, &token_program_id, Some(&order));
        let prepare_swap_out_escrow =
            self.prepare_associated_token_account(swap_out_token, &token_program_id, Some(&order));
        let prepare_ata = self.prepare_associated_token_account(
            swap_out_token,
            &token_program_id,
            Some(&receiver),
        );

        let create = self
            .treasury_transaction()
            .anchor_args(args::CreateSwap {
                nonce,
                swap_path_length: swap_path
                    .len()
                    .try_into()
                    .map_err(|_| crate::Error::custom("swap path is too long"))?,
                swap_in_amount: swap_in_token_amount,
                min_swap_out_amount: options.min_swap_out_amount,
            })
            .anchor_accounts(accounts::CreateSwap {
                authority: self.payer(),
                store: *store,
                config,
                treasury_vault_config,
                swap_in_token: *swap_in_token,
                swap_out_token: *swap_out_token,
                swap_in_token_receiver_vault,
                market,
                receiver,
                user,
                swap_in_token_escrow,
                swap_out_token_escrow,
                order,
                store_program: *self.store_program_id(),
                token_program: token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            })
            .accounts(swap_path);

        Ok(prepare_ata
            .merge(prepare_swap_in_escrow)
            .merge(prepare_swap_out_escrow)
            .merge(create)
            .output(order))
    }

    async fn cancel_treasury_swap(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<(&Pubkey, &Pubkey)>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let config = self.find_treasury_config_address(store);
        let receiver = self.find_treasury_receiver_address(&config);
        let user = self.find_user_address(store, &receiver);

        let (swap_in_token, swap_out_token) = match hint {
            Some((swap_in_token, swap_out_token)) => (*swap_in_token, *swap_out_token),
            None => {
                let order = self.order(order).await?;
                let swap_in_token = order.tokens.initial_collateral.token().ok_or_else(|| {
                    crate::Error::custom("invalid swap order: missing swap in token")
                })?;

                let swap_out_token = order.tokens.final_output_token.token().ok_or_else(|| {
                    crate::Error::custom("invalid swap order: missing swap out token")
                })?;
                (swap_in_token, swap_out_token)
            }
        };

        // Currently only SPL-Token is supported.
        let token_program_id = anchor_spl::token::ID;

        let swap_in_token_receiver_vault = get_associated_token_address_with_program_id(
            &receiver,
            &swap_in_token,
            &token_program_id,
        );
        let swap_out_token_receiver_vault = get_associated_token_address_with_program_id(
            &receiver,
            &swap_out_token,
            &token_program_id,
        );
        let swap_in_token_escrow =
            get_associated_token_address_with_program_id(order, &swap_in_token, &token_program_id);
        let swap_out_token_escrow =
            get_associated_token_address_with_program_id(order, &swap_out_token, &token_program_id);

        let prepare = self.prepare_associated_token_account(
            &swap_out_token,
            &token_program_id,
            Some(&receiver),
        );

        let cancel = self
            .treasury_transaction()
            .anchor_args(args::CancelSwap {})
            .anchor_accounts(accounts::CancelSwap {
                authority: self.payer(),
                store: *store,
                store_wallet: self.find_store_wallet_address(store),
                config,
                receiver,
                user,
                swap_in_token,
                swap_out_token,
                swap_in_token_receiver_vault,
                swap_out_token_receiver_vault,
                swap_in_token_escrow,
                swap_out_token_escrow,
                order: *order,
                event_authority: self.store_event_authority(),
                store_program: *self.store_program_id(),
                token_program: token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            });

        Ok(prepare.merge(cancel))
    }
}

/// Create Treasury Swap Options.
#[derive(Debug, Clone, Default)]
pub struct CreateTreasurySwapOptions {
    /// Nonce.
    pub nonce: Option<NonceBytes>,
    /// The market tokens of the swap path.
    pub swap_path: Vec<Pubkey>,
    /// Min swap out amount.
    pub min_swap_out_amount: Option<u64>,
    /// Hint for the treasury vault config address.
    pub treasury_vault_config_hint: Option<Pubkey>,
}

/// Confirm GT buyback builder.
pub struct ConfirmGtBuybackBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    gt_exchange_vault: Pubkey,
    oracle: Pubkey,
    with_chainlink_program: bool,
    feeds_parser: FeedsParser,
    hint: Option<ConfirmGtBuybackHint>,
}

/// Hint for confirming GT buyback.
#[derive(Debug, Clone)]
pub struct ConfirmGtBuybackHint {
    config: Pubkey,
    treasury_vault_config: Pubkey,
    token_map: Pubkey,
    treasury_tokens: Vec<Pubkey>,
    feeds: TokensWithFeed,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ConfirmGtBuybackBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        gt_exchange_vault: &Pubkey,
        oracle: &Pubkey,
    ) -> Self {
        Self {
            client,
            store: *store,
            gt_exchange_vault: *gt_exchange_vault,
            oracle: *oracle,
            with_chainlink_program: false,
            feeds_parser: Default::default(),
            hint: None,
        }
    }

    /// Prepare [`ConfirmGtBuybackHint`].
    pub async fn prepare_hint(&mut self) -> crate::Result<ConfirmGtBuybackHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let (config, treasury_vault_config_address) =
                    find_config_addresses(self.client, &self.store, None).await?;
                let gt_bank = self
                    .client
                    .find_gt_bank_address(&treasury_vault_config_address, &self.gt_exchange_vault);
                let map_address = self
                    .client
                    .authorized_token_map_address(&self.store)
                    .await?
                    .ok_or_else(|| crate::Error::custom("token map is not set"))?;
                let map = self.client.token_map(&map_address).await?;
                let gt_bank = self
                    .client
                    .account::<ZeroCopy<GtBank>>(&gt_bank)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let treasury_vault_config = self
                    .client
                    .account::<ZeroCopy<TreasuryVaultConfig>>(&treasury_vault_config_address)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let hint = ConfirmGtBuybackHint {
                    config,
                    treasury_vault_config: treasury_vault_config_address,
                    token_map: map_address,
                    treasury_tokens: treasury_vault_config.tokens().collect(),
                    feeds: gt_bank.to_feeds(&map, &treasury_vault_config)?,
                };
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    async fn build_txn(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;

        let gt_bank = self
            .client
            .find_gt_bank_address(&hint.treasury_vault_config, &self.gt_exchange_vault);

        let token_program_id = anchor_spl::token::ID;

        let feeds = self.feeds_parser.parse_and_sort_by_tokens(&hint.feeds)?;
        let tokens = hint.treasury_tokens.iter().map(|pubkey| AccountMeta {
            pubkey: *pubkey,
            is_signer: false,
            is_writable: false,
        });
        let vaults = hint.treasury_tokens.iter().map(|token| {
            let pubkey = get_associated_token_address_with_program_id(
                &hint.treasury_vault_config,
                token,
                &token_program_id,
            );
            AccountMeta {
                pubkey,
                is_signer: false,
                is_writable: false,
            }
        });

        let chainlink_program = if self.with_chainlink_program {
            Some(program_ids::CHAINLINK)
        } else {
            None
        };

        let rpc = self
            .client
            .treasury_transaction()
            .anchor_args(args::ConfirmGtBuyback {})
            .accounts(fix_optional_account_metas(
                accounts::ConfirmGtBuyback {
                    authority: self.client.payer(),
                    store: self.store,
                    config: hint.config,
                    treasury_vault_config: hint.treasury_vault_config,
                    gt_exchange_vault: self.gt_exchange_vault,
                    gt_bank,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    event_authority: self.client.store_event_authority(),
                    store_program: *self.client.store_program_id(),
                    chainlink_program,
                },
                &ID,
                self.client.treasury_program_id(),
            ))
            .accounts(feeds)
            .accounts(tokens.chain(vaults).collect::<Vec<_>>());

        Ok(rpc)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for ConfirmGtBuybackBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        let mut tx = self.client.bundle_with_options(options);
        tx.try_push(
            self.build_txn()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?,
        )?;

        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for ConfirmGtBuybackBuilder<'_, C>
{
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(self.store, hint.feeds))
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.feeds_parser
            .insert_pull_oracle_feed_parser(provider, map);
        Ok(())
    }
}

impl<C> SetExecutionFee for ConfirmGtBuybackBuilder<'_, C> {
    fn set_execution_fee(&mut self, _lamports: u64) -> &mut Self {
        self
    }
}

async fn find_config_addresses<C: Deref<Target = impl Signer> + Clone>(
    client: &crate::Client<C>,
    store: &Pubkey,
    treasury_vault_config: Option<&Pubkey>,
) -> crate::Result<(Pubkey, Pubkey)> {
    let config = client.find_treasury_config_address(store);
    match treasury_vault_config {
        Some(address) => Ok((config, *address)),
        None => {
            let config_account = client
                .account::<ZeroCopy<Config>>(&config)
                .await?
                .ok_or(crate::Error::NotFound)?
                .0;
            Ok((
                config,
                *optional_address(&config_account.treasury_vault_config)
                    .ok_or_else(|| crate::Error::custom("treasury vault config is not set"))?,
            ))
        }
    }
}
