use std::time::Duration;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_programs::gmsol_treasury::client::{
    accounts as treasury_accounts, args as treasury_args,
};
use gmsol_sdk::client::ops::{
    GtOps, SystemProgramOps, TokenAccountOps, TokenConfigOps, TreasuryOps, UserOps,
};
use gmsol_sdk::utils::zero_copy::ZeroCopy;
use gmsol_solana_utils::signer::{shared_signer, SignerRef};
use gmsol_store::{constants::DEFAULT_GT_VAULT_TIME_WINDOW, CoreError};
use gmsol_utils::gt::get_time_window_index;
use gmsol_utils::token_config::{TokenFlag, UpdateTokenConfigParams};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_instruction,
};

use crate::anchor_test::setup::{current_deployment, Deployment, GT_EXCHANGE_WINDOW_LOCK};

async fn assert_completion_rejected_atomically(
    caller: &gmsol_sdk::Client<SignerRef>,
    deployment: &Deployment,
    instruction: Instruction,
    exchange: &Pubkey,
    bank: &Pubkey,
    receiver: &Pubkey,
    owner: &Pubkey,
    tokens: &[Pubkey],
) -> eyre::Result<Option<u32>> {
    let rpc = caller.rpc();
    let before_exchange = rpc.get_account(exchange).await?;
    let before_bank = rpc.get_account(bank).await?;
    let before_receiver = rpc.get_balance(receiver).await?;
    let mut before_tokens = Vec::with_capacity(tokens.len());
    for token in tokens {
        before_tokens.push(deployment.get_ata_amount(token, owner).await?);
    }
    let result = caller
        .treasury_transaction()
        .accounts(instruction.accounts)
        .args(instruction.data)
        .send()
        .await;
    let error = result.expect_err("invalid completion unexpectedly succeeded");
    assert_eq!(rpc.get_account(exchange).await?, before_exchange);
    assert_eq!(rpc.get_account(bank).await?, before_bank);
    assert_eq!(rpc.get_balance(receiver).await?, before_receiver);
    for (token, amount) in tokens.iter().zip(before_tokens) {
        assert_eq!(deployment.get_ata_amount(token, owner).await?, amount);
    }
    Ok(gmsol_sdk::Error::from(error).anchor_error_code())
}

#[tokio::test]
async fn permissionless_treasury_completion_refunds_recorded_receiver() -> eyre::Result<()> {
    const WINDOW: u32 = 30;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let owner = admin.try_clone_with_payer(shared_signer(Keypair::new()))?;
    let caller = deployment.user_client(Deployment::DEFAULT_USER)?;
    let rent_payer = Keypair::new();
    let token = deployment.token("WSOL").expect("WSOL token must exist");
    let store = &deployment.store;
    let config = admin.find_treasury_config_address(store);
    let treasury_receiver = admin.find_treasury_receiver_address(&config);

    admin.transfer(&owner.payer(), 50_000_000)?.send().await?;
    admin
        .transfer(&rent_payer.pubkey(), 10_000_000)?
        .send()
        .await?;
    let vault_config = admin.find_treasury_vault_config_address(&config, 0);
    deployment
        .mint_or_transfer_to("WSOL", &treasury_receiver, 1)
        .await?;

    keeper
        .gt_set_exchange_time_window(store, WINDOW)
        .send()
        .await?;
    let result = async {
        owner.prepare_user(store)?.send().await?;
        let clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
            .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let remaining = i64::from(WINDOW) - clock.unix_timestamp.rem_euclid(i64::from(WINDOW));
        if remaining < 20 {
            tokio::time::sleep(Duration::from_secs((remaining + 1) as u64)).await;
        }
        let clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
            .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let index = get_time_window_index(clock.unix_timestamp, i64::from(WINDOW));
        let vault = keeper.find_gt_exchange_vault_address(store, index, WINDOW);
        let exchange = keeper.find_gt_exchange_address(&vault, &owner.payer());
        keeper
            .prepare_gt_exchange_vault_with_time_window_index(store, index, WINDOW)
            .send()
            .await?;
        keeper
            .prepare_gt_bank(store, Some(&vault_config), &vault)
            .await?
            .send()
            .await?;
        let bank = keeper.find_gt_bank_address(&vault_config, &vault);
        admin
            .prepare_associated_token_account(
                &token.address,
                &anchor_spl::token::ID,
                Some(&vault_config),
            )
            .send()
            .await?;
        admin
            .prepare_associated_token_account(&token.address, &anchor_spl::token::ID, Some(&bank))
            .send()
            .await?;
        let ata = |authority| {
            anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(
                authority,
                &token.address,
            )
        };
        admin
            .treasury_transaction()
            .anchor_accounts(treasury_accounts::DepositToTreasuryVault {
                authority: admin.payer(),
                store: *store,
                config,
                treasury_vault_config: vault_config,
                receiver: treasury_receiver,
                gt_exchange_vault: vault,
                gt_bank: bank,
                token: token.address,
                receiver_vault: ata(&treasury_receiver),
                treasury_vault: ata(&vault_config),
                gt_bank_vault: ata(&bank),
                store_program: *admin.store_program_id(),
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(treasury_args::DepositToTreasuryVault {})
            .send()
            .await?;
        owner
            .request_gt_exchange_v2_with_time_window_index(
                store,
                &owner.payer(),
                &owner.payer(),
                &rent_payer.pubkey(),
                index,
                WINDOW,
                0,
            )
            .signer(&rent_payer)
            .send()
            .await?;

        loop {
            let clock_account = keeper
                .rpc()
                .get_account(&solana_sdk::sysvar::clock::id())
                .await?;
            let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
                .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
            if get_time_window_index(clock.unix_timestamp, i64::from(WINDOW)) > index {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let mut confirm = keeper.confirm_gt_buyback(store, &vault, &deployment.treasury_oracle());
        deployment
            .execute_with_pyth(&mut confirm, None, true, true)
            .await?;

        let rent = caller.rpc().get_balance(&exchange).await?;
        let receiver_before = caller.rpc().get_balance(&rent_payer.pubkey()).await?;
        assert!(rent > 0);
        assert_eq!(deployment.get_ata_amount(&token.address, &owner.payer()).await?, None);
        assert!(caller
            .complete_gt_exchange_v2(
                store,
                &exchange,
                Some(&vault_config),
                Some(vec![(token.address, anchor_spl::token::ID)]),
                Some(&vault),
            )
            .await?
            .send()
            .await
            .is_err());
        assert_eq!(caller.rpc().get_balance(&exchange).await?, rent);
        caller
            .complete_gt_exchange_v2(
                store,
                &exchange,
                Some(&vault_config),
                Some(vec![(token.address, anchor_spl::token::ID)]),
                Some(&vault),
            )
            .await?
            .pre_instruction(
                anchor_spl::associated_token::spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                    &caller.payer(), &owner.payer(), &token.address, &anchor_spl::token::ID,
                ),
                true,
            )
            .send()
            .await?;
        assert_eq!(deployment.get_ata_amount(&token.address, &owner.payer()).await?, Some(0));
        assert_eq!(caller.rpc().get_balance(&exchange).await?, 0);
        assert_eq!(
            caller.rpc().get_balance(&rent_payer.pubkey()).await?,
            receiver_before + rent
        );
        Ok::<(), eyre::Report>(())
    }
    .await;

    let restore_window = keeper
        .gt_set_exchange_time_window(store, DEFAULT_GT_VAULT_TIME_WINDOW)
        .send()
        .await;
    result?;
    restore_window?;
    Ok(())
}

#[tokio::test]
async fn permissionless_completion_uses_recorded_bank_balance_and_order() -> eyre::Result<()> {
    const WINDOW: u32 = 30;
    const CLAIM_GT: u64 = 100 * 10_000_000;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let caller = deployment.user_client(Deployment::DEFAULT_USER)?;
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let fallback_owner = Keypair::new();
    let rent_payer = Keypair::new();
    let store = &deployment.store;
    let wsol_mint = deployment
        .token("WSOL")
        .expect("WSOL token must exist")
        .address;
    let isolated_mint = loop {
        let mint = Keypair::new();
        if mint.pubkey().to_bytes() < wsol_mint.to_bytes() {
            break mint;
        }
    };
    let btc = deployment.token("fBTC").expect("fBTC token must exist");
    let mint_rent = admin
        .rpc()
        .get_minimum_balance_for_rent_exemption(
            <anchor_spl::token::spl_token::state::Mint as Pack>::LEN,
        )
        .await?;
    admin
        .store_transaction()
        .signer(&isolated_mint)
        .pre_instruction(
            system_instruction::create_account(
                &admin.payer(),
                &isolated_mint.pubkey(),
                mint_rent,
                <anchor_spl::token::spl_token::state::Mint as Pack>::LEN as u64,
                &anchor_spl::token::ID,
            ),
            true,
        )
        .pre_instruction(
            anchor_spl::token::spl_token::instruction::initialize_mint2(
                &anchor_spl::token::ID,
                &isolated_mint.pubkey(),
                &admin.payer(),
                Some(&admin.payer()),
                btc.config.decimals,
            )?,
            true,
        )
        .send()
        .await?;
    let token_map = admin
        .authorized_token_map_address(store)
        .await?
        .expect("fixture token map must exist");
    let token_config = UpdateTokenConfigParams::default()
        .update_price_feed(&btc.config.provider, btc.config.feed_id, None)?
        .with_expected_provider(btc.config.provider)
        .with_precision(btc.config.precision);
    keeper
        .insert_token_config(
            store,
            &token_map,
            "isolated-btc",
            &isolated_mint.pubkey(),
            token_config,
            true,
            true,
        )
        .send()
        .await?;
    let tokens = [wsol_mint, btc.address, isolated_mint.pubkey()];
    let config = admin.find_treasury_config_address(store);
    let treasury_receiver = admin.find_treasury_receiver_address(&config);
    let vault_config = admin.find_treasury_vault_config_address(&config, 0);
    admin
        .insert_token_to_treasury(store, Some(&vault_config), &tokens[2])
        .await?
        .send()
        .await?;
    admin
        .toggle_token_flag(
            store,
            Some(&vault_config),
            &tokens[2],
            TokenFlag::AllowDeposit,
            true,
        )
        .await?
        .send()
        .await?;

    for owner in &owners {
        admin.transfer(&owner.pubkey(), 50_000_000)?.send().await?;
    }
    admin
        .transfer(&fallback_owner.pubkey(), 50_000_000)?
        .send()
        .await?;
    admin
        .transfer(&rent_payer.pubkey(), 50_000_000)?
        .send()
        .await?;
    admin
        .set_gt_factor(store, gmsol_store::constants::MARKET_USD_UNIT)?
        .send()
        .await?;
    admin
        .set_buyback_factor(store, gmsol_store::constants::MARKET_USD_UNIT / 2)?
        .send()
        .await?;

    keeper
        .gt_set_exchange_time_window(store, WINDOW)
        .send()
        .await?;
    let result = async {
        for owner in &owners {
            let client = admin.try_clone_with_payer(shared_signer(Keypair::from_bytes(&owner.to_bytes())?))?;
            client.prepare_user(store)?.send().await?;
            keeper.mint_gt_reward(store, &owner.pubkey(), 2 * CLAIM_GT)?.send().await?;
            for token in tokens {
                client.prepare_associated_token_account(&token, &anchor_spl::token::ID, None)
                    .send().await?;
            }
        }
        let fallback_client = admin.try_clone_with_payer(shared_signer(Keypair::from_bytes(&fallback_owner.to_bytes())?))?;
        fallback_client.prepare_user(store)?.send().await?;
        for token in tokens {
            fallback_client.prepare_associated_token_account(&token, &anchor_spl::token::ID, None)
                .send().await?;
        }

        for order in [[0_usize, 1, 2], [2, 0, 1]] {
            let clock_account = keeper.rpc().get_account(&solana_sdk::sysvar::clock::id()).await?;
            let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
                .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
            let remaining = i64::from(WINDOW) - clock.unix_timestamp.rem_euclid(i64::from(WINDOW));
            if remaining < 20 {
                tokio::time::sleep(Duration::from_secs((remaining + 1) as u64)).await;
            }
            let clock_account = keeper.rpc().get_account(&solana_sdk::sysvar::clock::id()).await?;
            let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
                .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
            let index = get_time_window_index(clock.unix_timestamp, i64::from(WINDOW));
            let vault = keeper.find_gt_exchange_vault_address(store, index, WINDOW);
            let bank = keeper.find_gt_bank_address(&vault_config, &vault);
            keeper.prepare_gt_exchange_vault_with_time_window_index(store, index, WINDOW)
                .send().await?;
            keeper.prepare_gt_bank(store, Some(&vault_config), &vault).await?.send().await?;
            for owner in &owners {
                let client = admin.try_clone_with_payer(shared_signer(Keypair::from_bytes(&owner.to_bytes())?))?;
                client.request_gt_exchange_v2_with_time_window_index(
                    store, &owner.pubkey(), &owner.pubkey(), &rent_payer.pubkey(), index, WINDOW, CLAIM_GT,
                ).signer(&rent_payer).send().await?;
            }
            fallback_client.request_gt_exchange_with_time_window_index(store, index, WINDOW, 0)
                .send().await?;
            for (token, amount) in [(tokens[0], 3_000_000_u64), (tokens[1], 0), (tokens[2], 3)] {
                admin.prepare_associated_token_account(&token, &anchor_spl::token::ID,
                    Some(&treasury_receiver)).send().await?;
                if token == tokens[0] {
                    deployment.mint_or_transfer_to("WSOL", &treasury_receiver, amount).await?;
                } else if token == tokens[2] {
                    let receiver_ata = anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(
                        &treasury_receiver, &token);
                    admin.store_transaction().pre_instruction(
                        anchor_spl::token::spl_token::instruction::mint_to_checked(
                            &anchor_spl::token::ID, &token, &receiver_ata, &admin.payer(),
                            &[], amount, btc.config.decimals,
                        )?, true).send().await?;
                }
                admin.prepare_associated_token_account(&token, &anchor_spl::token::ID, Some(&vault_config))
                    .send().await?;
                admin.prepare_associated_token_account(&token, &anchor_spl::token::ID, Some(&bank))
                    .send().await?;
                let ata = |authority| anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(authority, &token);
                admin.treasury_transaction()
                    .anchor_accounts(treasury_accounts::DepositToTreasuryVault {
                        authority: admin.payer(), store: *store, config,
                        treasury_vault_config: vault_config, receiver: treasury_receiver,
                        gt_exchange_vault: vault, gt_bank: bank, token,
                        receiver_vault: ata(&treasury_receiver), treasury_vault: ata(&vault_config),
                        gt_bank_vault: ata(&bank), store_program: *admin.store_program_id(),
                        token_program: anchor_spl::token::ID,
                        associated_token_program: anchor_spl::associated_token::ID,
                    })
                    .anchor_args(treasury_args::DepositToTreasuryVault {})
                    .send().await?;
            }
            let bank_tokens = caller.account::<ZeroCopy<gmsol_programs::gmsol_treasury::accounts::GtBank>>(&bank)
                .await?.expect("funded bank must exist").0.tokens().collect::<Vec<_>>();
            assert_eq!(bank_tokens.len(), tokens.len());
            assert!(tokens.iter().all(|token| bank_tokens.contains(token)));
            let wsol_index = bank_tokens.iter().position(|token| *token == wsol_mint)
                .expect("WSOL must be in the bank");
            assert!(wsol_index > 0, "isolated mint must precede WSOL in bank order");

            let first_exchange = keeper.find_gt_exchange_address(&vault, &owners[order[0]].pubkey());
            let first_instruction = caller.complete_gt_exchange_v2(
                store, &first_exchange, Some(&vault_config),
                Some(bank_tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                Some(&vault),
            ).await?.take_instruction().expect("completion instruction");
            assert_completion_rejected_atomically(
                &caller, deployment, first_instruction, &first_exchange, &bank,
                &rent_payer.pubkey(), &owners[order[0]].pubkey(), &tokens,
            ).await?;
            loop {
                let clock_account = keeper.rpc().get_account(&solana_sdk::sysvar::clock::id()).await?;
                let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
                    .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
                if get_time_window_index(clock.unix_timestamp, i64::from(WINDOW)) > index { break; }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            let mut confirm = keeper.confirm_gt_buyback(store, &vault, &deployment.treasury_oracle());
            deployment.execute_with_pyth(&mut confirm, None, true, true).await?;
            let confirmed = caller.account::<ZeroCopy<gmsol_programs::gmsol_treasury::accounts::GtBank>>(&bank)
                .await?.expect("confirmed bank must exist").0;
            assert_eq!(confirmed.tokens().collect::<Vec<_>>(), bank_tokens,
                "bank token order must remain stable through confirmation");
            let mut bank_balances = tokens.map(|token| confirmed.balances.data.iter()
                .find(|entry| entry.key == token.to_bytes())
                .expect("bank token must be recorded").value.amount);
            assert!(bank_balances[0] >= 3, "WSOL must retain a positive per-owner payout");
            assert_eq!(bank_balances[1], 0, "unfunded fBTC stays zero");
            assert_eq!(bank_balances[2], 1,
                "fBTC must retain one unit for three equal GT claims");

            let first_owner = owners[order[0]].pubkey();
            let base = caller.complete_gt_exchange_v2(
                store, &first_exchange, Some(&vault_config),
                Some(bank_tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                Some(&vault),
            ).await?.take_instruction().expect("completion instruction");
            assert_eq!(base.accounts.len(), 20);
            assert_eq!(base.accounts[0].pubkey, first_owner);
            assert_eq!(base.accounts[1].pubkey, rent_payer.pubkey());
            assert!(!base.accounts[0].is_signer);
            assert!(base.accounts[1].is_writable);
            assert_eq!([base.accounts[11].pubkey, base.accounts[12].pubkey,
                base.accounts[13].pubkey], bank_tokens.as_slice());
            for (index, token) in bank_tokens.iter().enumerate() {
                let bank_ata = anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(
                    &bank, token);
                let owner_ata = anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(
                    &first_owner, token);
                assert_eq!(base.accounts[14 + index].pubkey, bank_ata);
                assert_eq!(base.accounts[17 + index].pubkey, owner_ata);
            }

            let noncanonical = Keypair::new();
            let token_rent = caller.rpc().get_minimum_balance_for_rent_exemption(
                <anchor_spl::token::spl_token::state::Account as Pack>::LEN,
            ).await?;
            admin.store_transaction().signer(&noncanonical)
                .pre_instruction(system_instruction::create_account(
                    &admin.payer(), &noncanonical.pubkey(), token_rent,
                    <anchor_spl::token::spl_token::state::Account as Pack>::LEN as u64,
                    &anchor_spl::token::ID,
                ), true)
                .pre_instruction(anchor_spl::token::spl_token::instruction::initialize_account3(
                    &anchor_spl::token::ID, &noncanonical.pubkey(), &tokens[0], &first_owner,
                )?, true)
                .send().await?;
            let mut invalid = Vec::new();
            let mut wrong_owner = base.clone();
            wrong_owner.accounts[0].pubkey = owners[order[1]].pubkey();
            invalid.push(("wrong owner", wrong_owner, Some(CoreError::InvalidArgument.into())));
            let mut wrong_receiver = base.clone();
            wrong_receiver.accounts[1].pubkey = admin.payer();
            invalid.push(("wrong receiver", wrong_receiver, Some(CoreError::InvalidArgument.into())));
            let mut noncanonical_target = base.clone();
            noncanonical_target.accounts[17 + wsol_index].pubkey = noncanonical.pubkey();
            invalid.push(("noncanonical target", noncanonical_target,
                Some(gmsol_programs::anchor_lang::error::ErrorCode::AccountNotAssociatedTokenAccount.into())));
            let mut wrong_mint = base.clone();
            wrong_mint.accounts[11 + wsol_index].pubkey = bank_tokens[(wsol_index + 1) % bank_tokens.len()];
            invalid.push(("wrong mint", wrong_mint, Some(CoreError::InvalidArgument.into())));
            let mut wrong_vault = base.clone();
            wrong_vault.accounts[14 + wsol_index].pubkey = base.accounts[17 + wsol_index].pubkey;
            invalid.push(("wrong vault", wrong_vault, None));
            let mut wrong_program = base.clone();
            wrong_program.accounts[9].pubkey = *admin.store_program_id();
            invalid.push(("wrong token program", wrong_program, None));
            let mut wrong_order = base.clone();
            wrong_order.accounts.swap(11, 12);
            invalid.push(("wrong mint order", wrong_order, Some(CoreError::InvalidArgument.into())));
            let mut missing_account = base.clone();
            missing_account.accounts.pop();
            invalid.push(("missing account", missing_account, Some(CoreError::InvalidArgument.into())));
            let mut extra_account = base.clone();
            extra_account.accounts.push(AccountMeta::new_readonly(admin.payer(), false));
            invalid.push(("extra account", extra_account, Some(CoreError::InvalidArgument.into())));
            for (case, instruction, expected_code) in invalid {
                let actual_code = assert_completion_rejected_atomically(
                    &caller, deployment, instruction, &first_exchange, &bank,
                    &rent_payer.pubkey(), &first_owner, &tokens,
                ).await?;
                if let Some(expected_code) = expected_code {
                    assert_eq!(actual_code, Some(expected_code), "{case}");
                }
            }

            let owner_client = admin.try_clone_with_payer(shared_signer(Keypair::from_bytes(&owners[order[0]].to_bytes())?))?;
            let legacy = owner_client.complete_gt_exchange(
                store, &first_exchange, Some(&vault_config),
                Some(bank_tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                Some(&vault),
            ).await?.take_instruction().expect("legacy completion instruction");
            assert_completion_rejected_atomically(
                &owner_client, deployment, legacy, &first_exchange, &bank,
                &rent_payer.pubkey(), &first_owner, &tokens,
            ).await?;

            let fallback_exchange = keeper.find_gt_exchange_address(&vault, &fallback_owner.pubkey());
            let fallback_rent = caller.rpc().get_balance(&fallback_exchange).await?;
            let fallback_before = caller.rpc().get_balance(&fallback_owner.pubkey()).await?;
            let fallback_bank_before = caller.rpc().get_account(&bank).await?;
            if order[0] == 0 {
                caller.complete_gt_exchange_v2(store, &fallback_exchange, None, None, None)
                    .await?.send().await?;
            } else {
                let legacy_fallback = fallback_client.complete_gt_exchange(
                    store, &fallback_exchange, Some(&vault_config),
                    Some(bank_tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                    Some(&vault),
                ).await?.take_instruction().expect("legacy completion instruction");
                caller.treasury_transaction().accounts(legacy_fallback.accounts)
                    .args(legacy_fallback.data).signer(&fallback_owner).send().await?;
            }
            assert_eq!(caller.rpc().get_balance(&fallback_exchange).await?, 0);
            assert_eq!(caller.rpc().get_balance(&fallback_owner.pubkey()).await?, fallback_before + fallback_rent);
            assert_eq!(caller.rpc().get_account(&bank).await?, fallback_bank_before);

            for (step, owner_index) in order.into_iter().enumerate() {
                let owner = owners[owner_index].pubkey();
                let exchange = keeper.find_gt_exchange_address(&vault, &owner);
                let remaining_gt = (3 - step as u64) * CLAIM_GT;
                let payouts = bank_balances.map(|balance| balance / (3 - step as u64));
                assert_eq!(payouts[2], u64::from(step == 2),
                    "only the last owner receives the fBTC unit");
                let isolated_ata = anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(
                    &owner, &tokens[2]);
                if step == 0 || step == 2 {
                    admin.store_transaction().pre_instruction(
                        anchor_spl::token::spl_token::instruction::freeze_account(
                            &anchor_spl::token::ID, &isolated_ata, &tokens[2],
                            &admin.payer(), &[],
                        )?, true).send().await?;
                }
                let mut before = Vec::with_capacity(tokens.len());
                for token in tokens {
                    before.push(deployment.get_ata_amount(&token, &owner).await?.unwrap_or(0));
                }
                let duplicate = caller.complete_gt_exchange_v2(
                    store, &exchange, Some(&vault_config),
                    Some(bank_tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                    Some(&vault),
                ).await?.take_instruction().expect("completion instruction");
                if step == 2 {
                    assert_completion_rejected_atomically(
                        &caller, deployment, duplicate.clone(), &exchange, &bank,
                        &rent_payer.pubkey(), &owner, &tokens,
                    ).await?;
                    admin.store_transaction().pre_instruction(
                        anchor_spl::token::spl_token::instruction::thaw_account(
                            &anchor_spl::token::ID, &isolated_ata, &tokens[2],
                            &admin.payer(), &[],
                        )?, true).send().await?;
                }
                caller.complete_gt_exchange_v2(
                    store, &exchange, Some(&vault_config),
                    Some(bank_tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                    Some(&vault),
                ).await?.send().await?;
                if step == 0 {
                    admin.store_transaction().pre_instruction(
                        anchor_spl::token::spl_token::instruction::thaw_account(
                            &anchor_spl::token::ID, &isolated_ata, &tokens[2],
                            &admin.payer(), &[],
                        )?, true).send().await?;
                }
                for (token_index, token) in tokens.iter().enumerate() {
                    assert_eq!(deployment.get_ata_amount(token, &owner).await?, Some(before[token_index] + payouts[token_index]));
                }
                let recorded = caller.account::<ZeroCopy<gmsol_programs::gmsol_treasury::accounts::GtBank>>(&bank)
                    .await?.expect("confirmed bank must exist").0;
                assert_eq!(recorded.remaining_confirmed_gt_amount, remaining_gt - CLAIM_GT);
                for (token_index, token) in tokens.iter().enumerate() {
                    let entry = recorded.balances.data.iter()
                        .find(|entry| entry.key == token.to_bytes())
                        .expect("bank token must be recorded");
                    bank_balances[token_index] -= payouts[token_index];
                    assert_eq!(entry.value.amount, bank_balances[token_index]);
                }
                assert_eq!(caller.rpc().get_balance(&exchange).await?, 0);
                let bank_after = caller.rpc().get_account(&bank).await?;
                let receiver_after = caller.rpc().get_balance(&rent_payer.pubkey()).await?;
                assert!(caller.treasury_transaction().accounts(duplicate.accounts)
                    .args(duplicate.data).send().await.is_err());
                assert_eq!(caller.rpc().get_account(&bank).await?, bank_after);
                assert_eq!(caller.rpc().get_balance(&rent_payer.pubkey()).await?, receiver_after);
                assert_eq!(caller.rpc().get_balance(&exchange).await?, 0);
            }
        }
        Ok::<(), eyre::Report>(())
    }.await;

    let restore_window = keeper
        .gt_set_exchange_time_window(store, DEFAULT_GT_VAULT_TIME_WINDOW)
        .send()
        .await;
    let restore_factor = admin.set_gt_factor(store, 0)?.send().await;
    let restore_buyback_factor = admin.set_buyback_factor(store, 0)?.send().await;
    let remove_isolated_token = admin
        .remove_token_from_treasury(store, Some(&vault_config), &tokens[2])
        .await?
        .send()
        .await;
    result?;
    restore_window?;
    restore_factor?;
    restore_buyback_factor?;
    remove_isolated_token?;
    Ok(())
}

#[tokio::test]
async fn close_gt_exchange_checks_receiver_and_controller() -> eyre::Result<()> {
    const EXCHANGE_WINDOW: u32 = 30;

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

        let clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
            .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let window_remaining = i64::from(EXCHANGE_WINDOW)
            - clock.unix_timestamp.rem_euclid(i64::from(EXCHANGE_WINDOW));
        if window_remaining < 10 {
            tokio::time::sleep(Duration::from_secs((window_remaining + 1) as u64)).await;
        }
        let clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
            .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let index = get_time_window_index(clock.unix_timestamp, i64::from(EXCHANGE_WINDOW));
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

        loop {
            let window_clock_account = keeper
                .rpc()
                .get_account(&solana_sdk::sysvar::clock::id())
                .await?;
            let window_clock: solana_sdk::clock::Clock =
                solana_sdk::account::from_account(&window_clock_account)
                    .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
            if get_time_window_index(window_clock.unix_timestamp, i64::from(EXCHANGE_WINDOW))
                > index
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
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
        let owner_balance_after_failed_close = keeper.rpc().get_balance(&owner_address).await?;

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
            owner_balance_after_failed_close + exchange_rent
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
