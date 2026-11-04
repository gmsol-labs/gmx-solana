use std::time::Duration;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_sdk::client::ops::{GtOps, UserOps};
use gmsol_store::{constants::DEFAULT_GT_VAULT_TIME_WINDOW, CoreError};
use gmsol_utils::gt::get_time_window_index;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;

use crate::anchor_test::setup::{current_deployment, Deployment, GT_EXCHANGE_WINDOW_LOCK};

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
