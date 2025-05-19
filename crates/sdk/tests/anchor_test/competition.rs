use gmsol_competition::{
    instruction::InitializeCompetition,
    states::{Competition, Participant, PARTICIPANT_SEED},
    ID as COMPETITION_PROGRAM_ID,
};
use gmsol_programs::anchor_lang::Key;
use gmsol_sdk::{
    client::ops::ExchangeOps, constants::MARKET_USD_UNIT, ops::exchange::callback::Callback,
};
use solana_sdk::{pubkey::Pubkey, system_program};
use time::OffsetDateTime;

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
    let competition = Pubkey::find_program_address(&[b"competition"], &COMPETITION_PROGRAM_ID).0;

    let slot = client.rpc().get_slot().await?;
    let start_time = client
        .rpc()
        .get_block_time(slot)
        .await
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp());
    let end_time = start_time + 3600; // 1 hour competition

    let init_competition = client
        .store_transaction()
        .program(COMPETITION_PROGRAM_ID)
        .anchor_args(InitializeCompetition {
            start_time,
            end_time,
            store_program: store.key(),
        })
        .anchor_accounts(gmsol_competition::accounts::InitializeCompetition {
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
    assert_eq!(competition_account.store_program, store.key());

    // Create and execute order
    let size = 5_000 * MARKET_USD_UNIT;

    // let action_kind = ActionKind::Order.into();
    let owner = client.payer();
    // let action_stats = Pubkey::find_program_address(
    //     &[ACTION_STATS_SEED, owner.as_ref(), &[action_kind]],
    //     &deployment.callback_program,
    // )
    // .0;

    // Create participant account first
    let participant = Pubkey::find_program_address(
        &[PARTICIPANT_SEED, competition.as_ref(), owner.as_ref()],
        &COMPETITION_PROGRAM_ID,
    )
    .0;

    let create_participant = client
        .store_transaction()
        .program(COMPETITION_PROGRAM_ID)
        .anchor_args(gmsol_competition::instruction::CreateParticipantIdempotent {})
        .anchor_accounts(gmsol_competition::accounts::CreateParticipantIdempotent {
            payer: client.payer(),
            competition,
            participant,
            trader: owner,
            system_program: system_program::ID,
        });

    // let signature = create_participant.send().await?;
    // tracing::info!(%signature, "created participant account");

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
            program: COMPETITION_PROGRAM_ID,
            config: competition,
            action_stats: participant,
        }))
        .build_with_address()
        .await?;

    rpc = create_participant.merge(rpc);
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an increase position order");

    // Execute order
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let oracle = &deployment.oracle();
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

    // Verify participant account creation
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

    // // Cancel order
    // let signature = client.close_order(&order)?.build().await?.send().await?;
    // tracing::info!(%order, %signature, "cancelled increase position order");

    Ok(())
}
