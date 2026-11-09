use std::time::Duration;

use anchor_spl::associated_token::{
    get_associated_token_address,
    spl_associated_token_account::instruction::create_associated_token_account_idempotent,
};
use eyre::OptionExt;
use gmsol_programs::{
    anchor_lang::error::ErrorCode,
    gmsol_store::client::{accounts, args},
    gmsol_treasury::{
        accounts::GtBank,
        client::{accounts as treasury_accounts, args as treasury_args},
    },
};
use gmsol_sdk::{
    client::ops::{GtOps, SystemProgramOps, TokenAccountOps, TreasuryOps, UserOps},
    utils::zero_copy::ZeroCopy,
    Client,
};
use gmsol_solana_utils::{signer::SignerRef, transaction_builder::TransactionBuilder};
use gmsol_store::{constants::DEFAULT_GT_VAULT_TIME_WINDOW, CoreError};
use gmsol_utils::gt::get_time_window_index;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::AccountMeta, pubkey::Pubkey,
    signature::Keypair, signer::Signer, system_instruction,
};

use crate::anchor_test::setup::{current_deployment, Deployment, GT_EXCHANGE_WINDOW_LOCK};

const TREASURY_WINDOW: u32 = 30;

async fn processed_validator_clock(rpc: &RpcClient) -> eyre::Result<solana_sdk::clock::Clock> {
    let account = rpc
        .get_account_with_commitment(
            &solana_sdk::sysvar::clock::id(),
            CommitmentConfig::processed(),
        )
        .await?
        .value
        .ok_or_else(|| eyre::eyre!("validator clock account missing"))?;
    solana_sdk::account::from_account(&account)
        .ok_or_else(|| eyre::eyre!("invalid validator clock"))
}

#[tokio::test]
async fn close_gt_exchange_checks_receiver_and_controller() -> eyre::Result<()> {
    const EXCHANGE_WINDOW: u32 = 30;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let owner = deployment.funded_client().await?;
    let second_owner = deployment.funded_client().await?;
    let store = &deployment.store;
    let owner_address = owner.payer();
    let second_owner_address = second_owner.payer();

    keeper
        .gt_set_exchange_time_window(store, EXCHANGE_WINDOW)
        .send()
        .await?;
    let result = async {
        owner.prepare_user(store)?.send().await?;
        second_owner.prepare_user(store)?.send().await?;
        keeper
            .mint_gt_reward(store, &owner_address, 2)?
            .send()
            .await?;
        keeper
            .mint_gt_reward(store, &second_owner_address, 2)?
            .send()
            .await?;

        let index =
            tokio::time::timeout(Duration::from_secs(u64::from(EXCHANGE_WINDOW) * 2), async {
                loop {
                    let clock = processed_validator_clock(keeper.rpc()).await?;
                    let window = i64::from(EXCHANGE_WINDOW);
                    let remaining = window - clock.unix_timestamp.rem_euclid(window);
                    if remaining >= 20 {
                        break Ok::<i64, eyre::Report>(get_time_window_index(
                            clock.unix_timestamp,
                            window,
                        ));
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
            .await
            .map_err(|_| {
                eyre::eyre!("validator clock did not provide exchange window headroom")
            })??;
        let vault = keeper.find_gt_exchange_vault_address(store, index, EXCHANGE_WINDOW);
        let exchange = keeper.find_gt_exchange_address(&vault, &owner_address);
        let second_exchange = keeper.find_gt_exchange_address(&vault, &second_owner_address);
        keeper
            .prepare_gt_exchange_vault_with_time_window_index(store, index, EXCHANGE_WINDOW)
            .send()
            .await?;
        owner
            .request_gt_exchange_with_time_window_index(store, index, EXCHANGE_WINDOW, 1)
            .send()
            .await?;
        second_owner
            .request_gt_exchange_with_time_window_index(store, index, EXCHANGE_WINDOW, 1)
            .send()
            .await?;

        tokio::time::timeout(Duration::from_secs(u64::from(EXCHANGE_WINDOW) * 2), async {
            loop {
                let clock = processed_validator_clock(keeper.rpc()).await?;
                if get_time_window_index(clock.unix_timestamp, i64::from(EXCHANGE_WINDOW)) > index {
                    break Ok::<(), eyre::Report>(());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
        .await
        .map_err(|_| eyre::eyre!("validator clock did not advance beyond exchange window"))??;
        keeper
            .confirm_gt_exchange_vault(store, &vault, 0, None)
            .send()
            .await?;

        let exchange_rent = keeper.rpc().get_balance(&exchange).await?;
        let owner_balance = keeper.rpc().get_balance(&owner_address).await?;
        let wrong_receiver = second_owner_address;
        let err = keeper
            .store_transaction()
            .anchor_accounts(accounts::CloseGtExchangeV2 {
                authority: keeper.payer(),
                store: *store,
                owner: owner_address,
                vault,
                rent_receiver: wrong_receiver,
                exchange,
            })
            .anchor_args(args::CloseGtExchangeV2 {})
            .send()
            .await
            .expect_err("an unrelated receiver must be rejected");
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(CoreError::RentReceiverMismatched.into())
        );
        assert_eq!(keeper.rpc().get_balance(&exchange).await?, exchange_rent);
        assert_eq!(
            keeper.rpc().get_balance(&owner_address).await?,
            owner_balance
        );

        owner
            .store_transaction()
            .anchor_accounts(accounts::CloseGtExchangeV2 {
                authority: owner_address,
                store: *store,
                owner: owner_address,
                vault,
                rent_receiver: owner_address,
                exchange,
            })
            .anchor_args(args::CloseGtExchangeV2 {})
            .send()
            .await
            .expect_err("the owner is not a GT controller");
        assert_eq!(keeper.rpc().get_balance(&exchange).await?, exchange_rent);

        keeper
            .store_transaction()
            .anchor_accounts(accounts::CloseGtExchangeV2 {
                authority: keeper.payer(),
                store: *store,
                owner: owner_address,
                vault,
                rent_receiver: owner_address,
                exchange,
            })
            .anchor_args(args::CloseGtExchangeV2 {})
            .send()
            .await?;
        assert_eq!(keeper.rpc().get_balance(&exchange).await?, 0);
        assert_eq!(
            keeper.rpc().get_balance(&owner_address).await?,
            owner_balance + exchange_rent
        );

        let second_exchange_rent = keeper.rpc().get_balance(&second_exchange).await?;
        let second_owner_balance = keeper.rpc().get_balance(&second_owner_address).await?;
        keeper
            .close_gt_exchange(
                store,
                &second_exchange,
                Some(&second_owner_address),
                Some(&vault),
            )
            .await?
            .send()
            .await?;
        assert_eq!(keeper.rpc().get_balance(&second_exchange).await?, 0);
        assert_eq!(
            keeper.rpc().get_balance(&second_owner_address).await?,
            second_owner_balance + second_exchange_rent
        );
        Ok::<(), eyre::Report>(())
    }
    .await;
    let restore = keeper
        .gt_set_exchange_time_window(store, DEFAULT_GT_VAULT_TIME_WINDOW)
        .send()
        .await;
    result?;
    restore?;
    Ok(())
}

#[tokio::test]
async fn legacy_treasury_completion_pays_owner_and_rejects_distinct_rent_receiver(
) -> eyre::Result<()> {
    const GT_AMOUNT: u64 = 1_000;
    const USDG_DEPOSIT: u64 = 1_000_000;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let tokens = treasury_tokens(deployment)?;
    let owner = deployment.funded_client().await?;
    let sponsored_owner = deployment.funded_client().await?;
    let rent_payer = Keypair::new();
    admin
        .transfer(&rent_payer.pubkey(), Deployment::FUNDED_CLIENT_LAMPORTS)?
        .send()
        .await?;

    keeper
        .gt_set_exchange_time_window(store, TREASURY_WINDOW)
        .send()
        .await?;
    let result = async {
        for client in [&owner, &sponsored_owner] {
            client.prepare_user(store)?.send().await?;
            keeper
                .mint_gt_reward(store, &client.payer(), GT_AMOUNT)?
                .send()
                .await?;
            for token in &tokens {
                admin
                    .prepare_associated_token_account(
                        token,
                        &anchor_spl::token::ID,
                        Some(&client.payer()),
                    )
                    .send()
                    .await?;
            }
        }
        let window =
            TreasuryWindow::open(deployment, &[("USDG", USDG_DEPOSIT), ("fBTC", 0)]).await?;
        owner
            .request_gt_exchange_with_time_window_index(
                store,
                window.index,
                TREASURY_WINDOW,
                GT_AMOUNT,
            )
            .send()
            .await?;
        sponsored_owner
            .request_gt_exchange_v2_with_time_window_index(
                store,
                &sponsored_owner.payer(),
                &sponsored_owner.payer(),
                &rent_payer.pubkey(),
                window.index,
                TREASURY_WINDOW,
                GT_AMOUNT,
            )
            .signer(&rent_payer)
            .send()
            .await?;
        window.confirm(deployment).await?;

        let sponsored_exchange =
            admin.find_gt_exchange_address(&window.vault, &sponsored_owner.payer());
        let code = reject_without_change(
            admin,
            sponsored_owner
                .complete_gt_exchange(store, &sponsored_exchange, None, None, None)
                .await?,
            &[sponsored_exchange, window.bank, rent_payer.pubkey()],
        )
        .await?;
        assert_eq!(code, Some(CoreError::RentReceiverMismatched.into()));

        let exchange = admin.find_gt_exchange_address(&window.vault, &owner.payer());
        let payouts = expected_payouts(&window.bank(admin).await?, &tokens, GT_AMOUNT);
        assert!(
            payouts.iter().any(|payout| *payout > 0),
            "the GT Bank must pay out"
        );
        owner
            .complete_gt_exchange(store, &exchange, None, None, None)
            .await?
            .send()
            .await?;
        assert_eq!(admin.rpc().get_balance(&exchange).await?, 0);
        for (token, payout) in tokens.iter().zip(payouts) {
            assert_eq!(
                deployment.get_ata_amount(token, &owner.payer()).await?,
                Some(payout)
            );
        }
        Ok::<(), eyre::Report>(())
    }
    .await;
    let restore = keeper
        .gt_set_exchange_time_window(store, DEFAULT_GT_VAULT_TIME_WINDOW)
        .send()
        .await;
    result?;
    restore?;
    Ok(())
}

#[tokio::test]
async fn permissionless_completion_pays_owner_atas_and_rent_receiver() -> eyre::Result<()> {
    const GT_AMOUNT: u64 = 1_000;
    const DUST_GT_AMOUNT: u64 = 1;
    /// Fewer USDG units than the total GT, so the USDG share of the dust exchange rounds to zero.
    const USDG_DEPOSIT: u64 = GT_AMOUNT;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let tokens = treasury_tokens(deployment)?;
    let usdg = deployment
        .token("USDG")
        .ok_or_eyre("USDG must exist")?
        .address;
    let dust_owner = deployment.funded_client().await?;
    let owner = deployment.funded_client().await?;
    let legacy_owner = deployment.funded_client().await?;
    let caller = deployment.funded_client().await?;
    let rent_payer = Keypair::new();
    admin
        .transfer(&rent_payer.pubkey(), Deployment::FUNDED_CLIENT_LAMPORTS)?
        .send()
        .await?;

    keeper
        .gt_set_exchange_time_window(store, TREASURY_WINDOW)
        .send()
        .await?;
    let result = async {
        for (client, amount) in [
            (&dust_owner, DUST_GT_AMOUNT),
            (&owner, GT_AMOUNT),
            (&legacy_owner, GT_AMOUNT),
        ] {
            client.prepare_user(store)?.send().await?;
            keeper
                .mint_gt_reward(store, &client.payer(), amount)?
                .send()
                .await?;
        }
        // The dust owner's associated token accounts are created only when completing.
        for client in [&owner, &legacy_owner, &caller] {
            for token in &tokens {
                admin
                    .prepare_associated_token_account(
                        token,
                        &anchor_spl::token::ID,
                        Some(&client.payer()),
                    )
                    .send()
                    .await?;
            }
        }
        let window =
            TreasuryWindow::open(deployment, &[("USDG", USDG_DEPOSIT), ("fBTC", 0)]).await?;
        dust_owner
            .request_gt_exchange_with_time_window_index(
                store,
                window.index,
                TREASURY_WINDOW,
                DUST_GT_AMOUNT,
            )
            .send()
            .await?;
        owner
            .request_gt_exchange_v2_with_time_window_index(
                store,
                &owner.payer(),
                &owner.payer(),
                &rent_payer.pubkey(),
                window.index,
                TREASURY_WINDOW,
                GT_AMOUNT,
            )
            .signer(&rent_payer)
            .send()
            .await?;
        legacy_owner
            .request_gt_exchange_with_time_window_index(
                store,
                window.index,
                TREASURY_WINDOW,
                GT_AMOUNT,
            )
            .send()
            .await?;
        window.confirm(deployment).await?;

        // fBTC has no recorded balance and the USDG share of the dust exchange
        // rounds to zero, but the owner's associated token accounts are still required.
        let bank = window.bank(admin).await?;
        assert!(expected_payouts(&bank, &tokens, DUST_GT_AMOUNT)
            .iter()
            .all(|payout| *payout == 0));
        assert!(recorded_balance(&bank, &usdg) > 0);
        assert!(tokens
            .iter()
            .any(|token| recorded_balance(&bank, token) == 0));
        let dust_exchange = admin.find_gt_exchange_address(&window.vault, &dust_owner.payer());
        let code = reject_without_change(
            admin,
            caller
                .complete_gt_exchange_v2(store, &dust_exchange, None, None, None, None)
                .await?,
            &[dust_exchange, window.bank],
        )
        .await?;
        assert_eq!(code, Some(ErrorCode::AccountNotInitialized.into()));

        // The caller can create them earlier in the same transaction.
        let mut complete = caller
            .complete_gt_exchange_v2(store, &dust_exchange, None, None, None, None)
            .await?;
        for token in &tokens {
            complete = complete.pre_instruction(
                create_associated_token_account_idempotent(
                    &caller.payer(),
                    &dust_owner.payer(),
                    token,
                    &anchor_spl::token::ID,
                ),
                true,
            );
        }
        let dust_owner_lamports = admin.rpc().get_balance(&dust_owner.payer()).await?;
        let rent = admin.rpc().get_balance(&dust_exchange).await?;
        complete.send().await?;
        for token in &tokens {
            assert_eq!(
                deployment
                    .get_ata_amount(token, &dust_owner.payer())
                    .await?,
                Some(0)
            );
        }
        assert_eq!(admin.rpc().get_balance(&dust_exchange).await?, 0);
        assert_eq!(
            admin.rpc().get_balance(&dust_owner.payer()).await?,
            dust_owner_lamports + rent
        );
        let bank_after = window.bank(admin).await?;
        for token in &tokens {
            assert_eq!(
                recorded_balance(&bank_after, token),
                recorded_balance(&bank, token)
            );
        }
        assert_eq!(
            bank_after.remaining_confirmed_gt_amount,
            bank.remaining_confirmed_gt_amount - DUST_GT_AMOUNT
        );

        // Payouts go only to the owner's associated token accounts, and the rent
        // only to the recorded rent receiver.
        let exchange = admin.find_gt_exchange_address(&window.vault, &owner.payer());
        let owner_address = owner.payer();
        let owner_ata = |token: &Pubkey| get_associated_token_address(&owner_address, token);
        let caller_address = caller.payer();
        let caller_ata = |token: &Pubkey| get_associated_token_address(&caller_address, token);
        let watched = [exchange, window.bank, rent_payer.pubkey()]
            .into_iter()
            .chain(tokens.iter().map(owner_ata))
            .collect::<Vec<_>>();
        let code = reject_without_change(
            admin,
            complete_gt_exchange_v2_to(
                &caller,
                deployment,
                &window,
                &owner_address,
                &caller_address,
                owner_ata,
            )
            .await?,
            &watched,
        )
        .await?;
        assert_eq!(code, Some(CoreError::RentReceiverMismatched.into()));
        let code = reject_without_change(
            admin,
            complete_gt_exchange_v2_to(
                &caller,
                deployment,
                &window,
                &owner_address,
                &rent_payer.pubkey(),
                caller_ata,
            )
            .await?,
            &watched,
        )
        .await?;
        assert_eq!(code, Some(ErrorCode::ConstraintTokenOwner.into()));
        let non_ata = Keypair::new();
        create_token_account(&owner, &non_ata, &usdg).await?;
        let code = reject_without_change(
            admin,
            complete_gt_exchange_v2_to(
                &caller,
                deployment,
                &window,
                &owner_address,
                &rent_payer.pubkey(),
                |token| {
                    if *token == usdg {
                        non_ata.pubkey()
                    } else {
                        owner_ata(token)
                    }
                },
            )
            .await?,
            &watched,
        )
        .await?;
        assert_eq!(
            code,
            Some(ErrorCode::AccountNotAssociatedTokenAccount.into())
        );

        let payouts = expected_payouts(&window.bank(admin).await?, &tokens, GT_AMOUNT);
        assert!(
            payouts.iter().any(|payout| *payout > 0),
            "the GT Bank must pay out"
        );
        let rent = admin.rpc().get_balance(&exchange).await?;
        let rent_payer_lamports = admin.rpc().get_balance(&rent_payer.pubkey()).await?;
        caller
            .complete_gt_exchange_v2(store, &exchange, None, None, None, None)
            .await?
            .send()
            .await?;
        for (token, payout) in tokens.iter().zip(payouts) {
            assert_eq!(
                deployment.get_ata_amount(token, &owner_address).await?,
                Some(payout)
            );
        }
        assert_eq!(admin.rpc().get_balance(&exchange).await?, 0);
        assert_eq!(
            admin.rpc().get_balance(&rent_payer.pubkey()).await?,
            rent_payer_lamports + rent
        );

        // The legacy completion follows the same rule, and the last exchange
        // receives the remaining recorded balance.
        let bank = window.bank(admin).await?;
        assert_eq!(bank.remaining_confirmed_gt_amount, GT_AMOUNT);
        let legacy_exchange = admin.find_gt_exchange_address(&window.vault, &legacy_owner.payer());
        legacy_owner
            .complete_gt_exchange(store, &legacy_exchange, None, None, None)
            .await?
            .send()
            .await?;
        for token in &tokens {
            assert_eq!(
                deployment
                    .get_ata_amount(token, &legacy_owner.payer())
                    .await?,
                Some(recorded_balance(&bank, token))
            );
        }
        Ok::<(), eyre::Report>(())
    }
    .await;
    let restore = keeper
        .gt_set_exchange_time_window(store, DEFAULT_GT_VAULT_TIME_WINDOW)
        .send()
        .await;
    result?;
    restore?;
    Ok(())
}

#[tokio::test]
async fn permissionless_completion_requires_confirmed_gt_bank() -> eyre::Result<()> {
    const GT_AMOUNT: u64 = 1;
    const NO_BUYBACK_VALUE: u128 = 0;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let owner = deployment.funded_client().await?;
    let caller = deployment.funded_client().await?;

    keeper
        .gt_set_exchange_time_window(store, TREASURY_WINDOW)
        .send()
        .await?;
    let result = async {
        owner.prepare_user(store)?.send().await?;
        keeper
            .mint_gt_reward(store, &owner.payer(), GT_AMOUNT)?
            .send()
            .await?;
        let window = TreasuryWindow::open(deployment, &[("USDG", 0)]).await?;
        owner
            .request_gt_exchange_with_time_window_index(
                store,
                window.index,
                TREASURY_WINDOW,
                GT_AMOUNT,
            )
            .send()
            .await?;
        window.wait_until_ended(admin).await?;
        // Confirm the GT exchange vault outside the treasury, leaving the GT Bank unconfirmed.
        keeper
            .confirm_gt_exchange_vault(store, &window.vault, NO_BUYBACK_VALUE, None)
            .send()
            .await?;

        let exchange = admin.find_gt_exchange_address(&window.vault, &owner.payer());
        let code = reject_without_change(
            admin,
            caller
                .complete_gt_exchange_v2(store, &exchange, None, None, None, None)
                .await?,
            &[exchange, window.bank, owner.payer()],
        )
        .await?;
        assert_eq!(code, Some(CoreError::PreconditionsAreNotMet.into()));
        Ok::<(), eyre::Report>(())
    }
    .await;
    let restore = keeper
        .gt_set_exchange_time_window(store, DEFAULT_GT_VAULT_TIME_WINDOW)
        .send()
        .await;
    result?;
    restore?;
    Ok(())
}

/// A GT exchange window whose GT Bank is funded through the treasury.
struct TreasuryWindow {
    index: i64,
    vault: Pubkey,
    bank: Pubkey,
}

impl TreasuryWindow {
    /// Enters a window with enough time left and deposits the given amounts of
    /// treasury tokens into its GT Bank.
    async fn open(deployment: &Deployment, deposits: &[(&str, u64)]) -> eyre::Result<Self> {
        const MIN_REMAINING_SECS: i64 = 20;

        let admin = &deployment.client;
        let store = &deployment.store;
        let window = i64::from(TREASURY_WINDOW);
        let remaining = window - validator_timestamp(admin).await?.rem_euclid(window);
        if remaining < MIN_REMAINING_SECS {
            tokio::time::sleep(Duration::from_secs((remaining + 1) as u64)).await;
        }
        let index = get_time_window_index(validator_timestamp(admin).await?, window);
        let vault = admin.find_gt_exchange_vault_address(store, index, TREASURY_WINDOW);
        let config = admin.find_treasury_config_address(store);
        let receiver = admin.find_treasury_receiver_address(&config);
        let vault_config = admin
            .find_treasury_vault_config_address(&config, Deployment::TREASURY_VAULT_CONFIG_INDEX);
        for (name, amount) in deposits {
            let token = deployment
                .token(name)
                .ok_or_eyre("treasury token must exist")?;
            deployment
                .mint_or_transfer_to(name, &receiver, *amount)
                .await?;
            let (deposit, deposit_vault) = admin
                .deposit_to_treasury_vault(
                    store,
                    Some(&vault_config),
                    &token.address,
                    None,
                    TREASURY_WINDOW,
                )
                .await?
                .swap_output(());
            eyre::ensure!(deposit_vault == vault, "the deposit left the window");
            deposit.send().await?;
        }
        Ok(Self {
            index,
            vault,
            bank: admin.find_gt_bank_address(&vault_config, &vault),
        })
    }

    async fn wait_until_ended(&self, client: &Client<SignerRef>) -> eyre::Result<()> {
        while get_time_window_index(
            validator_timestamp(client).await?,
            i64::from(TREASURY_WINDOW),
        ) <= self.index
        {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    /// Waits for the window to end and confirms its GT Bank.
    async fn confirm(&self, deployment: &Deployment) -> eyre::Result<()> {
        let admin = &deployment.client;
        self.wait_until_ended(admin).await?;
        let mut confirm = admin.confirm_gt_buyback(
            &deployment.store,
            &self.vault,
            &deployment.treasury_oracle(),
        );
        deployment
            .execute_with_pyth(&mut confirm, None, true, true)
            .await?;
        Ok(())
    }

    async fn bank(&self, client: &Client<SignerRef>) -> eyre::Result<GtBank> {
        Ok(client
            .account::<ZeroCopy<GtBank>>(&self.bank)
            .await?
            .ok_or_eyre("GT Bank must exist")?
            .0)
    }
}

/// floor(recorded GT Bank balance x GT amount / remaining confirmed GT amount) for each token.
fn expected_payouts(bank: &GtBank, tokens: &[Pubkey], gt_amount: u64) -> Vec<u64> {
    tokens
        .iter()
        .map(|token| {
            let payout = u128::from(recorded_balance(bank, token)) * u128::from(gt_amount)
                / u128::from(bank.remaining_confirmed_gt_amount);
            u64::try_from(payout).expect("a payout never exceeds the balance")
        })
        .collect()
}

fn recorded_balance(bank: &GtBank, token: &Pubkey) -> u64 {
    bank.balances.get(token).map_or(0, |balance| balance.amount)
}

/// Sends a transaction that must fail and checks that it leaves the given accounts unchanged.
async fn reject_without_change(
    client: &Client<SignerRef>,
    txn: TransactionBuilder<'_, SignerRef>,
    accounts: &[Pubkey],
) -> eyre::Result<Option<u32>> {
    let rpc = client.rpc();
    let before = rpc.get_multiple_accounts(accounts).await?;
    let err = txn.send().await.expect_err("the transaction must fail");
    assert_eq!(rpc.get_multiple_accounts(accounts).await?, before);
    Ok(gmsol_sdk::Error::from(err).anchor_error_code())
}

/// Builds `complete_gt_exchange_v2` for the owner's exchange in the window with the
/// given rent receiver, paying each GT Bank token to `target(token)`.
async fn complete_gt_exchange_v2_to<'a>(
    caller: &'a Client<SignerRef>,
    deployment: &Deployment,
    window: &TreasuryWindow,
    owner: &Pubkey,
    rent_receiver: &Pubkey,
    target: impl Fn(&Pubkey) -> Pubkey,
) -> eyre::Result<TransactionBuilder<'a, SignerRef>> {
    let store = &deployment.store;
    let config = caller.find_treasury_config_address(store);
    let treasury_vault_config =
        caller.find_treasury_vault_config_address(&config, Deployment::TREASURY_VAULT_CONFIG_INDEX);
    let tokens = window.bank(caller).await?.tokens().collect::<Vec<_>>();
    let mints = tokens
        .iter()
        .map(|token| AccountMeta::new_readonly(*token, false));
    let vaults = tokens
        .iter()
        .map(|token| AccountMeta::new(get_associated_token_address(&window.bank, token), false));
    let targets = tokens
        .iter()
        .map(|token| AccountMeta::new(target(token), false));
    Ok(caller
        .treasury_transaction()
        .anchor_accounts(treasury_accounts::CompleteGtExchangeV2 {
            store: *store,
            config,
            treasury_vault_config,
            gt_exchange_vault: window.vault,
            gt_bank: window.bank,
            owner: *owner,
            rent_receiver: *rent_receiver,
            exchange: caller.find_gt_exchange_address(&window.vault, owner),
            store_program: *caller.store_program_id(),
            token_program: anchor_spl::token::ID,
            token_2022_program: anchor_spl::token_2022::ID,
        })
        .anchor_args(treasury_args::CompleteGtExchangeV2 {})
        .accounts(mints.chain(vaults).chain(targets).collect()))
}

/// Creates a token account of `mint` owned by `owner` that is not its associated token account.
async fn create_token_account(
    owner: &Client<SignerRef>,
    account: &Keypair,
    mint: &Pubkey,
) -> eyre::Result<()> {
    use anchor_spl::token::spl_token::{
        instruction::initialize_account3, solana_program::program_pack::Pack, state::Account,
    };

    let rent = owner
        .rpc()
        .get_minimum_balance_for_rent_exemption(Account::LEN)
        .await?;
    owner
        .store_transaction()
        .signer(account)
        .pre_instruction(
            system_instruction::create_account(
                &owner.payer(),
                &account.pubkey(),
                rent,
                Account::LEN as u64,
                &anchor_spl::token::ID,
            ),
            true,
        )
        .pre_instruction(
            initialize_account3(
                &anchor_spl::token::ID,
                &account.pubkey(),
                mint,
                &owner.payer(),
            )?,
            true,
        )
        .send()
        .await?;
    Ok(())
}

fn treasury_tokens(deployment: &Deployment) -> eyre::Result<Vec<Pubkey>> {
    Deployment::TREASURY_TOKENS
        .iter()
        .map(|name| {
            deployment
                .token(name)
                .map(|token| token.address)
                .ok_or_eyre("treasury token must exist")
        })
        .collect()
}

async fn validator_timestamp(client: &Client<SignerRef>) -> eyre::Result<i64> {
    let account = client
        .rpc()
        .get_account(&solana_sdk::sysvar::clock::id())
        .await?;
    let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&account)
        .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
    Ok(clock.unix_timestamp)
}
