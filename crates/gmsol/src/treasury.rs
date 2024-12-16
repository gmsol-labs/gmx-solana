use std::{future::Future, ops::Deref};

use anchor_client::{
    anchor_lang::{prelude::AccountMeta, system_program, Id},
    solana_client::rpc_config::RpcAccountInfoConfig,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_store::states::{common::TokensWithFeed, gt::GtExchange, Chainlink, PriceProviderKind};
use gmsol_treasury::{
    accounts, instruction,
    states::{treasury::TokenFlag, Config, TreasuryConfig},
};
use solana_account_decoder::UiAccountEncoding;

use crate::{
    store::{gt::GtOps, utils::FeedsParser},
    utils::{
        builder::{
            FeedAddressMap, FeedIds, MakeTransactionBuilder, PullOraclePriceConsumer,
            SetExecutionFee,
        },
        fix_optional_account_metas, RpcBuilder, TransactionBuilder, ZeroCopy,
    },
};

/// Treasury instructions.
pub trait TreasuryOps<C> {
    /// Initialize [`Config`](crate::types::treasury::Config) account.
    fn initialize_config(&self, store: &Pubkey) -> RpcBuilder<C, Pubkey>;

    /// Set treasury.
    fn set_treasury(&self, store: &Pubkey, treasury_config: &Pubkey) -> RpcBuilder<C>;

    /// Set GT factor.
    fn set_gt_factor(&self, store: &Pubkey, factor: u128) -> crate::Result<RpcBuilder<C>>;

    /// Initialize [`TreasuryConfig`](crate::types::treasury::TreasuryConfig).
    fn initialize_treasury(&self, store: &Pubkey, index: u8) -> RpcBuilder<C, Pubkey>;

    /// Insert token to treasury.
    fn insert_token_to_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Remove token from treasury.
    fn remove_token_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Toggle token flag.
    fn toggle_token_flag(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Deposit into a treasury vault.
    fn deposit_into_treasury_valut(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        time_window: u32,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C, Pubkey>>>;

    /// Withdraw from a treasury vault.
    #[allow(clippy::too_many_arguments)]
    fn withdraw_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        amount: u64,
        decimals: u8,
        target: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Confirm GT buyback.
    fn confirm_gt_buyback(
        &self,
        store: &Pubkey,
        gt_exchange_vault: &Pubkey,
        oracle: &Pubkey,
    ) -> ConfirmGtBuybackBuilder<C>;

    /// Transfer receiver.
    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> RpcBuilder<C>;

    /// Set referral reward factors.
    fn set_referral_reward(&self, store: &Pubkey, factors: Vec<u128>) -> RpcBuilder<C>;

    /// Claim fees to receiver vault.
    fn claim_fees_to_receiver_vault(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        token_mint: &Pubkey,
    ) -> RpcBuilder<C>;

    /// Prepare GT bank.
    fn prepare_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C, Pubkey>>>;

    /// Sync GT bank.
    fn sync_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Complete GT exchange.
    fn complete_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        tokens_hint: Option<Vec<(Pubkey, Pubkey)>>,
        gt_exchange_vault_hint: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;
}

impl<S, C> TreasuryOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn initialize_config(&self, store: &Pubkey) -> RpcBuilder<C, Pubkey> {
        let config = self.find_config_address(store);
        self.treasury_rpc()
            .args(instruction::InitializeConfig {})
            .accounts(accounts::InitializeConfig {
                payer: self.payer(),
                store: *store,
                config,
                system_program: system_program::ID,
            })
            .with_output(config)
    }

    fn set_treasury(&self, store: &Pubkey, treasury_config: &Pubkey) -> RpcBuilder<C> {
        let config = self.find_config_address(store);
        self.treasury_rpc()
            .args(instruction::SetTreasury {})
            .accounts(accounts::SetTreasury {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config: *treasury_config,
                store_program: *self.store_program_id(),
            })
    }

    fn set_gt_factor(&self, store: &Pubkey, factor: u128) -> crate::Result<RpcBuilder<C>> {
        if factor > crate::constants::MARKET_USD_UNIT {
            return Err(crate::Error::invalid_argument(
                "cannot use a factor greater than 1",
            ));
        }
        let config = self.find_config_address(store);
        Ok(self
            .treasury_rpc()
            .args(instruction::SetGtFactor { factor })
            .accounts(accounts::SetGtFactor {
                authority: self.payer(),
                store: *store,
                config,
                store_program: *self.store_program_id(),
            }))
    }

    fn initialize_treasury(&self, store: &Pubkey, index: u8) -> RpcBuilder<C, Pubkey> {
        let config = self.find_config_address(store);
        let treasury_config = self.find_treasury_config_address(&config, index);
        self.treasury_rpc()
            .args(instruction::InitializeTreasury { index })
            .accounts(accounts::InitializeTreasury {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .with_output(treasury_config)
    }

    async fn insert_token_to_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        let (config, treasury_config) = find_config_addresses(self, store, treasury_config).await?;
        Ok(self
            .treasury_rpc()
            .args(instruction::InsertTokenToTreasury {})
            .accounts(accounts::InsertTokenToTreasury {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                token: *token_mint,
                store_program: *self.store_program_id(),
            }))
    }

    async fn remove_token_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        let (config, treasury_config) = find_config_addresses(self, store, treasury_config).await?;
        Ok(self
            .treasury_rpc()
            .args(instruction::RemoveTokenFromTreasury {})
            .accounts(accounts::RemoveTokenFromTreasury {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                token: *token_mint,
                store_program: *self.store_program_id(),
            }))
    }

    async fn toggle_token_flag(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> crate::Result<RpcBuilder<C>> {
        let (config, treasury_config) = find_config_addresses(self, store, treasury_config).await?;
        Ok(self
            .treasury_rpc()
            .args(instruction::ToggleTokenFlag {
                flag: flag.to_string(),
                value,
            })
            .accounts(accounts::ToggleTokenFlag {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                token: *token_mint,
                store_program: *self.store_program_id(),
            }))
    }

    async fn deposit_into_treasury_valut(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        time_window: u32,
    ) -> crate::Result<RpcBuilder<C, Pubkey>> {
        let (config, treasury_config) =
            find_config_addresses(self, store, treasury_config_hint).await?;

        let (prepare_gt_exchange_vault, gt_exchange_vault) = self
            .prepare_gt_exchange_vault_with_time_window(store, time_window)?
            .swap_output(());

        let (prepare_gt_bank, gt_bank) = self
            .prepare_gt_bank(store, Some(&treasury_config), &gt_exchange_vault)
            .await?
            .swap_output(());

        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);

        let receiver_vault =
            get_associated_token_address_with_program_id(&config, token_mint, token_program_id);
        let treasury_vault = get_associated_token_address_with_program_id(
            &treasury_config,
            token_mint,
            token_program_id,
        );
        let gt_bank_vault =
            get_associated_token_address_with_program_id(&gt_bank, token_mint, token_program_id);

        let deposit = self
            .treasury_rpc()
            .args(instruction::DepositIntoTreasury {})
            .accounts(accounts::DepositIntoTreasury {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                gt_exchange_vault,
                gt_bank,
                token: *token_mint,
                receiver_vault,
                treasury_vault,
                gt_bank_vault,
                store_program: *self.store_program_id(),
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            });
        Ok(prepare_gt_exchange_vault
            .merge(prepare_gt_bank)
            .merge(deposit)
            .with_output(gt_exchange_vault))
    }

    async fn withdraw_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        amount: u64,
        decimals: u8,
        target: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);

        let (config, treasury_config) =
            find_config_addresses(self, store, treasury_config_hint).await?;

        let treasury_vault = get_associated_token_address_with_program_id(
            &treasury_config,
            token_mint,
            token_program_id,
        );

        Ok(self
            .treasury_rpc()
            .args(instruction::WithdrawFromTreasury { amount, decimals })
            .accounts(accounts::WithdrawFromTreasury {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
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

    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> RpcBuilder<C> {
        self.treasury_rpc()
            .args(instruction::TransferReceiver {})
            .accounts(accounts::TransferReceiver {
                authority: self.payer(),
                store: *store,
                config: self.find_config_address(store),
                receiver: *new_receiver,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
    }

    fn set_referral_reward(&self, store: &Pubkey, factors: Vec<u128>) -> RpcBuilder<C> {
        self.treasury_rpc()
            .args(instruction::SetReferralReward { factors })
            .accounts(accounts::SetReferralReward {
                authority: self.payer(),
                store: *store,
                config: self.find_config_address(store),
                store_program: *self.store_program_id(),
            })
    }

    fn claim_fees_to_receiver_vault(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        token_mint: &Pubkey,
    ) -> RpcBuilder<C> {
        let config = self.find_config_address(store);
        let token_program_id = anchor_spl::token::ID;
        let receiver_vault =
            get_associated_token_address_with_program_id(&config, token_mint, &token_program_id);
        self.treasury_rpc()
            .args(instruction::ClaimFees {})
            .accounts(accounts::ClaimFees {
                authority: self.payer(),
                store: *store,
                config,
                market: self.find_market_address(store, market_token),
                token: *token_mint,
                vault: self.find_market_vault_address(store, token_mint),
                receiver_vault,
                store_program: *self.store_program_id(),
                token_program: token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            })
    }

    async fn prepare_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
    ) -> crate::Result<RpcBuilder<C, Pubkey>> {
        let (config, treasury_config) =
            find_config_addresses(self, store, treasury_config_hint).await?;
        let gt_bank = self.find_gt_bank_address(&treasury_config, gt_exchange_vault);
        Ok(self
            .treasury_rpc()
            .args(instruction::PrepareGtBank {})
            .accounts(accounts::PrepareGtBank {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                gt_exchange_vault: *gt_exchange_vault,
                gt_bank,
                store_program: *self.store_program_id(),
                system_program: system_program::ID,
            })
            .with_output(gt_bank))
    }

    async fn sync_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        let (config, treasury_config) =
            find_config_addresses(self, store, treasury_config_hint).await?;
        let gt_bank = self.find_gt_bank_address(&treasury_config, gt_exchange_vault);
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);

        let treasury_vault = get_associated_token_address_with_program_id(
            &treasury_config,
            token_mint,
            token_program_id,
        );
        let gt_bank_vault =
            get_associated_token_address_with_program_id(&gt_bank, token_mint, token_program_id);
        Ok(self
            .treasury_rpc()
            .args(instruction::SyncGtBank {})
            .accounts(accounts::SyncGtBank {
                authority: self.payer(),
                store: *store,
                config,
                treasury_config,
                gt_bank,
                token: *token_mint,
                treasury_vault,
                gt_bank_vault,
                store_program: *self.store_program_id(),
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }))
    }

    async fn complete_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        tokens_hint: Option<Vec<(Pubkey, Pubkey)>>,
        gt_exchange_vault_hint: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        let owner = self.payer();
        let (config, treasury_config) =
            find_config_addresses(self, store, treasury_config_hint).await?;
        let gt_exchange_vault = match gt_exchange_vault_hint {
            Some(address) => *address,
            None => {
                let exchange = self
                    .account::<ZeroCopy<GtExchange>>(exchange)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                *exchange.vault()
            }
        };
        let gt_bank = self.find_gt_bank_address(&treasury_config, &gt_exchange_vault);

        let tokens = match tokens_hint {
            Some(tokens) => tokens,
            None => {
                let treasury_config = self
                    .account::<ZeroCopy<TreasuryConfig>>(&treasury_config)
                    .await?
                    .ok_or_else(|| crate::Error::invalid_argument("treasury config not exist"))?
                    .0;

                let tokens = treasury_config.tokens().collect::<Vec<_>>();
                self.treasury_program()
                    .solana_rpc()
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
                    .map_err(crate::Error::invalid_argument)?
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
            .treasury_rpc()
            .args(instruction::CompleteGtExchange {})
            .accounts(accounts::CompleteGtExchange {
                owner,
                store: *store,
                config,
                treasury_config,
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
}

async fn find_config_addresses<C: Deref<Target = impl Signer> + Clone>(
    client: &crate::Client<C>,
    store: &Pubkey,
    treasury_config: Option<&Pubkey>,
) -> crate::Result<(Pubkey, Pubkey)> {
    let config = client.find_config_address(store);
    match treasury_config {
        Some(address) => Ok((config, *address)),
        None => {
            let config_account = client
                .account::<ZeroCopy<Config>>(&config)
                .await?
                .ok_or(crate::Error::NotFound)?
                .0;
            Ok((
                config,
                *config_account
                    .treasury_config()
                    .ok_or_else(|| crate::Error::invalid_argument("treasury config is not set"))?,
            ))
        }
    }
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
    treasury_config: Pubkey,
    token_map: Pubkey,
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
                let (config, treasury_config_address) =
                    find_config_addresses(self.client, &self.store, None).await?;
                let map_address = self
                    .client
                    .authorized_token_map_address(&self.store)
                    .await?
                    .ok_or_else(|| crate::Error::invalid_argument("token map is not set"))?;
                let map = self.client.token_map(&map_address).await?;
                let treasury_config = self
                    .client
                    .account::<ZeroCopy<TreasuryConfig>>(&treasury_config_address)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let hint = ConfirmGtBuybackHint {
                    config,
                    treasury_config: treasury_config_address,
                    token_map: map_address,
                    feeds: treasury_config.to_feeds(&map)?,
                };
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeTransactionBuilder<'a, C>
    for ConfirmGtBuybackBuilder<'a, C>
{
    async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;

        let gt_bank = self
            .client
            .find_gt_bank_address(&hint.treasury_config, &self.gt_exchange_vault);

        let chainlink_program = if self.with_chainlink_program {
            Some(Chainlink::id())
        } else {
            None
        };

        let feeds = self
            .feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()?;

        let rpc = self
            .client
            .treasury_rpc()
            .args(instruction::ConfirmGtBuyback {})
            .accounts(fix_optional_account_metas(
                accounts::ConfirmGtBuyback {
                    authority: self.client.payer(),
                    store: self.store,
                    config: hint.config,
                    treasury_config: hint.treasury_config,
                    gt_exchange_vault: self.gt_exchange_vault,
                    gt_bank,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    store_program: *self.client.store_program_id(),
                    chainlink_program,
                },
                &gmsol_treasury::ID,
                self.client.treasury_program_id(),
            ))
            .accounts(feeds);

        let mut tx = self.client.transaction();
        tx.try_push(rpc)?;

        Ok(tx)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for ConfirmGtBuybackBuilder<'a, C>
{
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(hint.feeds)
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

impl<'a, C> SetExecutionFee for ConfirmGtBuybackBuilder<'a, C> {
    fn set_execution_fee(&mut self, _lamports: u64) -> &mut Self {
        self
    }
}
