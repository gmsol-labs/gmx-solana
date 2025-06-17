use gmsol_callback::{accounts, instruction, interface::ActionKind, states::ACTION_STATS_SEED};
use gmsol_programs::gmsol_store::types::UpdateOrderParams;
use gmsol_sdk::{
    builders::callback::Callback, client::ops::ExchangeOps, constants::MARKET_USD_UNIT,
};
use solana_sdk::{pubkey::Pubkey, system_program};

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn callback() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("callback");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let fbtc = deployment.token("fBTC").expect("must exist");
    // let usdg = deployment.token("USDG").expect("must exist");

    let long_token_amount = 1_000_007;
    let short_token_amount = 6_000_000_000_011;

    let market_token = deployment
        .prepare_market(
            ["fBTC", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let long_collateral_amount = 100_005;
    let swap_in_amount = 100_007;
    // let short_collateral_amount = 103 * 100_000_000;

    deployment
        .mint_or_transfer_to_user(
            "fBTC",
            Deployment::DEFAULT_USER,
            long_collateral_amount + swap_in_amount,
        )
        .await?;
    // deployment
    //     .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, short_collateral_amount)
    //     .await?;

    let size = 5_000 * MARKET_USD_UNIT;
    let price = 400_000 * MARKET_USD_UNIT / 10u128.pow(fbtc.config.decimals as u32);

    let action_kind = ActionKind::Order.into();
    let owner = client.payer();
    let action_stats = Pubkey::find_program_address(
        &[ACTION_STATS_SEED, owner.as_ref(), &[action_kind]],
        &deployment.callback_program,
    )
    .0;

    // Create order.
    let (mut rpc, order) = client
        .market_increase(
            store,
            market_token,
            true,
            long_collateral_amount,
            true,
            size,
        )
        .callback(Some(
            Callback::builder()
                .version(0)
                .partitioned_data(action_stats)
                .shared_data(deployment.callback_shared_data)
                .program(deployment.callback_program)
                .build(),
        ))
        .build_with_address()
        .await?;
    let prepare_action_stats = client
        .store_transaction()
        .program(deployment.callback_program)
        .anchor_args(instruction::CreateActionStatsIdempotent { action_kind })
        .anchor_accounts(accounts::CreateActionStatsIdempotent {
            payer: client.payer(),
            action_stats,
            owner,
            system_program: system_program::ID,
        });
    rpc = prepare_action_stats.merge(rpc);
    let signature = rpc.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "created an increase position order");

    // Cancel order.
    let signature = client.close_order(&order)?.build().await?.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "cancelled increase position order");

    // Create order.
    let (mut rpc, order) = client
        .limit_increase(
            store,
            market_token,
            true,
            size,
            price,
            true,
            long_collateral_amount,
        )
        .callback(Some(
            Callback::builder()
                .version(0)
                .partitioned_data(action_stats)
                .shared_data(deployment.callback_shared_data)
                .program(deployment.callback_program)
                .build(),
        ))
        .build_with_address()
        .await?;
    let prepare_action_stats = client
        .store_transaction()
        .program(deployment.callback_program)
        .anchor_args(instruction::CreateActionStatsIdempotent { action_kind })
        .anchor_accounts(accounts::CreateActionStatsIdempotent {
            payer: client.payer(),
            action_stats,
            owner,
            system_program: system_program::ID,
        });
    rpc = prepare_action_stats.merge(rpc);
    let signature = rpc.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "created an increase position order");

    // Update order.
    let signature = client
        .update_order(
            store,
            market_token,
            &order,
            UpdateOrderParams {
                trigger_price: Some(price / 2),
                ..Default::default()
            },
            None,
        )
        .await?
        .send()
        .await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "updated increase position order");

    // Cancel order without callback.
    let signature = client
        .close_order(&order)?
        .skip_callback(true)
        .build()
        .await?
        .send()
        .await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "cancelled increase position order");

    // Create market increase order.
    let (mut rpc, order) = client
        .market_increase(
            store,
            market_token,
            true,
            long_collateral_amount,
            true,
            size,
        )
        .callback(Some(
            Callback::builder()
                .version(0)
                .partitioned_data(action_stats)
                .shared_data(deployment.callback_shared_data)
                .program(deployment.callback_program)
                .build(),
        ))
        .build_with_address()
        .await?;
    let prepare_action_stats = client
        .store_transaction()
        .program(deployment.callback_program)
        .anchor_args(instruction::CreateActionStatsIdempotent { action_kind })
        .anchor_accounts(accounts::CreateActionStatsIdempotent {
            payer: client.payer(),
            action_stats,
            owner,
            system_program: system_program::ID,
        });
    rpc = prepare_action_stats.merge(rpc);
    let signature = rpc.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "created an increase position order");

    // Execute market increase order.
    let mut builder = keeper.execute_order(store, oracle, &order, false)?;
    deployment
        .execute_with_pyth(
            builder
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .await?;

    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "executed order");

    // Create swap order.
    let (mut rpc, order) = client
        .market_swap(
            store,
            market_token,
            false,
            &fbtc.address,
            swap_in_amount,
            [market_token],
        )
        .callback(Some(
            Callback::builder()
                .version(0)
                .partitioned_data(action_stats)
                .shared_data(deployment.callback_shared_data)
                .program(deployment.callback_program)
                .build(),
        ))
        .build_with_address()
        .await?;
    let prepare_action_stats = client
        .store_transaction()
        .program(deployment.callback_program)
        .anchor_args(instruction::CreateActionStatsIdempotent { action_kind })
        .anchor_accounts(accounts::CreateActionStatsIdempotent {
            payer: client.payer(),
            action_stats,
            owner,
            system_program: system_program::ID,
        });
    rpc = prepare_action_stats.merge(rpc);
    let signature = rpc.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "created a swap order");

    // Execute swap order.
    let mut builder = keeper.execute_order(store, oracle, &order, false)?;
    deployment
        .execute_with_pyth(
            builder
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .await?;

    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "executed order");

    // Create market decrease order 1.
    let (mut rpc, order) = client
        .market_decrease(store, market_token, true, 0, true, size / 2)
        .callback(Some(
            Callback::builder()
                .version(0)
                .partitioned_data(action_stats)
                .shared_data(deployment.callback_shared_data)
                .program(deployment.callback_program)
                .build(),
        ))
        .build_with_address()
        .await?;
    let prepare_action_stats = client
        .store_transaction()
        .program(deployment.callback_program)
        .anchor_args(instruction::CreateActionStatsIdempotent { action_kind })
        .anchor_accounts(accounts::CreateActionStatsIdempotent {
            payer: client.payer(),
            action_stats,
            owner,
            system_program: system_program::ID,
        });
    rpc = prepare_action_stats.merge(rpc);
    let signature = rpc.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "created a decrease position order");

    // Execute market decrease order 1.
    let mut builder = keeper.execute_order(store, oracle, &order, false)?;
    deployment
        .execute_with_pyth(
            builder
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .await?;

    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "executed order");

    // Create market decrease order 2.
    let (mut rpc, order) = client
        .market_decrease(store, market_token, true, 0, true, size / 2)
        .callback(Some(
            Callback::builder()
                .version(0)
                .partitioned_data(action_stats)
                .shared_data(deployment.callback_shared_data)
                .program(deployment.callback_program)
                .build(),
        ))
        .build_with_address()
        .await?;
    let prepare_action_stats = client
        .store_transaction()
        .program(deployment.callback_program)
        .anchor_args(instruction::CreateActionStatsIdempotent { action_kind })
        .anchor_accounts(accounts::CreateActionStatsIdempotent {
            payer: client.payer(),
            action_stats,
            owner,
            system_program: system_program::ID,
        });
    rpc = prepare_action_stats.merge(rpc);
    let signature = rpc.send().await?;
    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "created a decrease position order");

    // Execute market decrease order 2.
    let mut builder = keeper.execute_order(store, oracle, &order, false)?;
    deployment
        .execute_with_pyth(
            builder
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .await?;

    let stats = client
        .account::<gmsol_callback::states::ActionStats>(&action_stats)
        .await?
        .expect("must exist");
    tracing::info!(%order, %signature, ?stats, "executed order");

    Ok(())
}
