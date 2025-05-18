use gmsol_callback::{accounts, instruction, interface::ActionKind, states::ACTION_STATS_SEED};
use gmsol_competition::{
    accounts::InitializeCompetition,
    states::{Competition, LeaderEntry, Participant},
};
use gmsol_sdk::{
    client::ops::ExchangeOps, constants::MARKET_USD_UNIT, ops::exchange::callback::Callback,
};
use solana_sdk::{pubkey::Pubkey, system_program};

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn competition() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("competition");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;

    // Prepare market
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

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, long_collateral_amount)
        .await?;

    // Initialize competition
    let competition = Pubkey::find_program_address(
        &[b"competition"],
        &deployment.competition_program,
    )
    .0;

    let start_time = Clock::get()?.unix_timestamp;
    let end_time = start_time + 3600; // 1 hour competition

    let init_competition = client
        .competition_transaction()
        .program(deployment.competition_program)
        .anchor_args(InitializeCompetition {
            start_time,
            end_time,
            store_program: store.program_id(),
        })
        .anchor_accounts(accounts::InitializeCompetition {
            payer: client.payer(),
            competition,
            system_program: system_program::ID,
        });

    let signature = init_competition.send().await?;
    tracing::info!(%signature, "initialized competition");

    // Verify competition initialization
    let competition_account = client
        .account::<Competition>(&competition)
        .await?
        .expect("must exist");
    assert!(competition_account.is_active);
    assert_eq!(competition_account.start_time, start_time);
    assert_eq!(competition_account.end_time, end_time);
    assert_eq!(competition_account.store_program, store.program_id());

    // Create and execute order
    let size = 5_000 * MARKET_USD_UNIT;

    let action_kind = ActionKind::Order.into();
    let owner = client.payer();
    let action_stats = Pubkey::find_program_address(
        &[ACTION_STATS_SEED, owner.as_ref(), &[action_kind]],
        &deployment.callback_program,
    )
    .0;

    // Create order
    let (mut rpc, order) = client
        .market_increase(
            store,
            market_token,
            true,
            long_collateral_amount,
            true,
            size,
        )
        .callback(Some(Callback {
            program: deployment.competition_program,
            config: deployment.competition_config,
            action_stats,
        }))
        .build_with_address()
        .await?;

    // Prepare action stats
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
    tracing::info!(%order, %signature, "created an increase position order");

    // Verify participant account creation
    let participant = Pubkey::find_program_address(
        &[b"participant", competition.as_ref(), owner.as_ref()],
        &deployment.competition_program,
    )
    .0;

    let participant_account = client
        .account::<Participant>(&participant)
        .await?
        .expect("must exist");
    assert_eq!(participant_account.owner, owner);
    assert_eq!(participant_account.competition, competition);

    // Verify leaderboard update
    let competition_account = client
        .account::<Competition>(&competition)
        .await?
        .expect("must exist");
    assert!(!competition_account.leaderboard.is_empty());
    let leader_entry = competition_account.leaderboard[0];
    assert_eq!(leader_entry.address, owner);
    assert!(leader_entry.volume > 0);

    // Cancel order
    let signature = client.close_order(&order)?.build().await?.send().await?;
    tracing::info!(%order, %signature, "cancelled increase position order");

    Ok(())
}
