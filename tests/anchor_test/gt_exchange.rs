use std::time::Duration;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_sdk::client::ops::{GtOps, SystemProgramOps, UserOps};
use gmsol_solana_utils::signer::shared_signer;
use gmsol_store::{constants::DEFAULT_GT_VAULT_TIME_WINDOW, CoreError};
use gmsol_utils::gt::get_time_window_index;
use solana_sdk::signature::Keypair;

use crate::anchor_test::setup::{current_deployment, Deployment, GT_EXCHANGE_WINDOW_LOCK};

#[tokio::test]
async fn scub_close_gt_exchange_checks_receiver_and_controller() -> eyre::Result<()> {
    const SCUB_WINDOW: u32 = 120;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let owner = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    let second_owner = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    let store = &deployment.store;
    let owner_address = owner.payer();
    let second_owner_address = second_owner.payer();

    for address in [owner_address, second_owner_address] {
        deployment
            .client
            .transfer(&address, 50_000_000)?
            .send()
            .await?;
    }

    keeper
        .gt_set_exchange_time_window(store, SCUB_WINDOW)
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

        let scub_clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let scub_clock: solana_sdk::clock::Clock =
            solana_sdk::account::from_account(&scub_clock_account)
                .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let window = i64::from(SCUB_WINDOW);
        let scub_remaining = window - scub_clock.unix_timestamp.rem_euclid(window);
        if scub_remaining < 30 {
            tokio::time::sleep(Duration::from_secs(u64::try_from(scub_remaining + 1)?)).await;
        }
        let scub_clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let scub_clock: solana_sdk::clock::Clock =
            solana_sdk::account::from_account(&scub_clock_account)
                .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let index = get_time_window_index(scub_clock.unix_timestamp, window);
        let vault = keeper.find_gt_exchange_vault_address(store, index, SCUB_WINDOW);
        let exchange = keeper.find_gt_exchange_address(&vault, &owner_address);
        let second_exchange = keeper.find_gt_exchange_address(&vault, &second_owner_address);
        keeper
            .prepare_gt_exchange_vault_with_time_window_index(store, index, SCUB_WINDOW)
            .send()
            .await?;
        owner
            .request_gt_exchange_with_time_window_index(store, index, SCUB_WINDOW, 1)
            .send()
            .await?;
        second_owner
            .request_gt_exchange_with_time_window_index(store, index, SCUB_WINDOW, 1)
            .send()
            .await?;

        loop {
            let scub_clock_account = keeper
                .rpc()
                .get_account(&solana_sdk::sysvar::clock::id())
                .await?;
            let scub_clock: solana_sdk::clock::Clock =
                solana_sdk::account::from_account(&scub_clock_account)
                    .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
            if get_time_window_index(scub_clock.unix_timestamp, i64::from(SCUB_WINDOW)) > index {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        keeper
            .confirm_gt_exchange_vault(store, &vault, 0, None)
            .send()
            .await?;

        let scub_rent = keeper.rpc().get_balance(&exchange).await?;
        let scub_owner_balance = keeper.rpc().get_balance(&owner_address).await?;
        let scub_wrong_receiver = second_owner_address;
        let err = keeper
            .store_transaction()
            .anchor_accounts(accounts::CloseGtExchangeV2 {
                authority: keeper.payer(),
                store: *store,
                owner: owner_address,
                vault,
                rent_receiver: scub_wrong_receiver,
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
        assert_eq!(keeper.rpc().get_balance(&exchange).await?, scub_rent);
        assert_eq!(
            keeper.rpc().get_balance(&owner_address).await?,
            scub_owner_balance
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
        assert_eq!(keeper.rpc().get_balance(&exchange).await?, scub_rent);

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
            scub_owner_balance + scub_rent
        );

        let scub_second_rent = keeper.rpc().get_balance(&second_exchange).await?;
        let scub_second_balance = keeper.rpc().get_balance(&second_owner_address).await?;
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
            scub_second_balance + scub_second_rent
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
