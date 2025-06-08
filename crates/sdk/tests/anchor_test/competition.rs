use gmsol_competition::{
    instruction::InitializeCompetition,
    states::{Competition, Participant, COMPETITION_SEED, PARTICIPANT_SEED},
    ID as COMPETITION_PROGRAM_ID,
};
use gmsol_sdk::{
    builders::callback::Callback, client::ops::ExchangeOps, constants::MARKET_USD_UNIT,
};
use rand::Rng;
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
    let payer = client.payer();
    let slot = client.rpc().get_slot().await?;
    let start_time = client
        .rpc()
        .get_block_time(slot)
        .await
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp());
    let competition = Pubkey::find_program_address(
        &[COMPETITION_SEED, payer.as_ref(), &start_time.to_le_bytes()],
        &COMPETITION_PROGRAM_ID,
    )
    .0;
    let end_time = start_time + 3600 * 24; // 24 hour competition
    let volume_threshold = 10_000 * MARKET_USD_UNIT; // 10,000 USD threshold
    let extension_duration = 10; // 10 seconds extension
    let extension_cap = 3600 * 24; // 24 hour maximum extension
    let only_count_increase = false;

    let init_competition = client
        .store_transaction()
        .program(COMPETITION_PROGRAM_ID)
        .anchor_args(InitializeCompetition {
            start_time,
            end_time,
            volume_threshold,
            extension_duration,
            extension_cap,
            only_count_increase,
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
    assert_eq!(competition_account.start_time, start_time);
    assert_eq!(competition_account.end_time, end_time);
    assert_eq!(competition_account.volume_threshold, volume_threshold);
    assert_eq!(competition_account.extension_duration, extension_duration);
    assert_eq!(competition_account.extension_cap, extension_cap);
    assert!(competition_account.extension_triggerer.is_none());

    // Create and execute order with volume exceeding threshold
    let size = 12_000 * MARKET_USD_UNIT; // 12,000 USD > 10,000 USD threshold

    let owner = client.payer();

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
        .callback(Some(
            Callback::builder()
                .version(0)
                .program(COMPETITION_PROGRAM_ID)
                .config(competition)
                .action_stats(participant)
                .build(),
        ))
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

    // Verify time extension
    let competition_account = client
        .account::<Competition>(&competition)
        .await?
        .expect("must exist");
    let participant_account = client
        .account::<Participant>(&participant)
        .await?
        .expect("must exist");
    let proposed_end_time = end_time + extension_duration;
    let max_end_time = participant_account.last_updated_at + extension_cap;
    assert_eq!(
        competition_account.end_time,
        proposed_end_time.min(max_end_time)
    );
    assert_eq!(competition_account.extension_triggerer, Some(owner));

    // Verify participant account creation
    let participant_account = client
        .account::<Participant>(&participant)
        .await?
        .expect("must exist");
    assert_eq!(participant_account.trader, owner);
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

    // Test with extra users.
    let mut rng = rand::thread_rng();
    for idx in 0..deployment.extra_user_count {
        let client = deployment.extra_user_client(idx)?;
        let owner = client.payer();

        deployment
            .mint_or_transfer_to("fBTC", &owner, long_collateral_amount)
            .await?;

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

        let random_size = rng.gen_range(10, 50) * MARKET_USD_UNIT;

        // Increase.
        let (mut rpc, order) = client
            .market_increase(
                store,
                market_token,
                true,
                long_collateral_amount,
                true,
                random_size,
            )
            .callback(Some(
                Callback::builder()
                    .version(0)
                    .program(COMPETITION_PROGRAM_ID)
                    .config(competition)
                    .action_stats(participant)
                    .build(),
            ))
            .build_with_address()
            .await?;

        rpc = create_participant.merge(rpc);
        let signature = rpc.send().await?;
        tracing::info!(extra_user=%idx, %order, %signature, "created order");

        let mut builder = keeper.execute_order(store, oracle, &order, false)?;
        // Ignore errors.
        _ = deployment
            .execute_with_pyth(
                builder
                    .add_alt(deployment.common_alt().clone())
                    .add_alt(deployment.market_alt().clone()),
                None,
                true,
                true,
            )
            .await;

        // Decrease.
        let (rpc, order) = client
            .market_decrease(store, market_token, true, 0, true, random_size)
            .callback(Some(
                Callback::builder()
                    .version(0)
                    .program(COMPETITION_PROGRAM_ID)
                    .config(competition)
                    .action_stats(participant)
                    .build(),
            ))
            .build_with_address()
            .await?;

        let signature = rpc.send().await?;
        tracing::info!(extra_user=%idx, %order, %signature, "created order");

        let mut builder = keeper.execute_order(store, oracle, &order, false)?;
        // Ignore errors.
        _ = deployment
            .execute_with_pyth(
                builder
                    .add_alt(deployment.common_alt().clone())
                    .add_alt(deployment.market_alt().clone()),
                None,
                true,
                true,
            )
            .await;
    }

    let competition_account = client
        .account::<Competition>(&competition)
        .await?
        .expect("must exist");

    tracing::info!("competition result: {competition_account:?}");

    Ok(())
}
