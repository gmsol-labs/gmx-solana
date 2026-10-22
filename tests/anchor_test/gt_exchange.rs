use std::time::Duration;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_programs::gmsol_treasury::client::{
    accounts as treasury_accounts, args as treasury_args,
};
use gmsol_sdk::client::ops::{
    GtOps, RoleOps, StoreOps, SystemProgramOps, TokenAccountOps, TreasuryOps, UserOps,
};
use gmsol_sdk::utils::zero_copy::ZeroCopy;
use gmsol_solana_utils::signer::{shared_signer, SignerRef};
use gmsol_store::{constants::DEFAULT_GT_VAULT_TIME_WINDOW, CoreError};
use gmsol_utils::{gt::get_time_window_index, token_config::TokenFlag};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{EncodableKey, Keypair},
    signer::Signer,
    system_instruction,
};

use crate::anchor_test::setup::{current_deployment, Deployment, GT_EXCHANGE_WINDOW_LOCK};

async fn prepare_treasury_for_completion_test(
    deployment: &Deployment,
    gt_factor: u128,
) -> eyre::Result<bool> {
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let config = admin.find_treasury_config_address(store);
    let vault_config = admin.find_treasury_vault_config_address(&config, 0);
    let initialized_now = admin
        .account::<ZeroCopy<gmsol_programs::gmsol_treasury::accounts::Config>>(&config)
        .await?
        .is_none();

    if initialized_now {
        for role in ["TREASURY_OWNER", "TREASURY_ADMIN", "TREASURY_KEEPER"] {
            admin.enable_role(store, role).send().await?;
            admin.grant_role(store, &admin.payer(), role).send().await?;
        }
        admin
            .grant_role(store, &keeper.payer(), "TREASURY_KEEPER")
            .send()
            .await?;
        let receiver = admin.find_treasury_receiver_address(&config);
        StoreOps::transfer_receiver(admin, store, &receiver)
            .send()
            .await?;
        admin.initialize_config(store).send().await?;
        admin
            .grant_role(store, &config, "GT_CONTROLLER")
            .send()
            .await?;
        admin
            .initialize_treasury_vault_config(store, 0)
            .send()
            .await?;
        admin
            .set_treasury_vault_config(store, &vault_config)
            .send()
            .await?;
        for name in ["WSOL", "fBTC"] {
            let token = deployment
                .token(name)
                .expect("fixture token must exist")
                .address;
            admin
                .insert_token_to_treasury(store, Some(&vault_config), &token)
                .await?
                .send()
                .await?;
            admin
                .toggle_token_flag(
                    store,
                    Some(&vault_config),
                    &token,
                    TokenFlag::AllowDeposit,
                    true,
                )
                .await?
                .send()
                .await?;
        }
    }

    let current_factor = admin
        .account::<ZeroCopy<gmsol_programs::gmsol_treasury::accounts::Config>>(&config)
        .await?
        .expect("treasury config must exist")
        .0
        .gt_factor;
    if current_factor != gt_factor {
        admin.set_gt_factor(store, gt_factor)?.send().await?;
    }
    Ok(initialized_now)
}

async fn fund_validator_payer_if_needed() -> eyre::Result<()> {
    let endpoint = std::env::var("ANCHOR_PROVIDER_URL")?;
    if endpoint.starts_with("http://127.0.0.1:") {
        let wallet_path = std::env::var("ANCHOR_WALLET")?;
        let wallet = shellexpand::full(&wallet_path)?;
        let payer = Keypair::read_from_file(wallet.as_ref())
            .map_err(|err| eyre::eyre!("could not read validator payer: {err}"))?;
        let rpc = solana_client::nonblocking::rpc_client::RpcClient::new(endpoint);
        if rpc.get_balance(&payer.pubkey()).await? == 0 {
            let signature = rpc
                .request_airdrop(&payer.pubkey(), 500_000_000_000)
                .await?;
            rpc.confirm_transaction(&signature).await?;
        }
    }
    Ok(())
}

async fn assert_completion_rejected_atomically(
    caller: &gmsol_sdk::Client<SignerRef>,
    deployment: &Deployment,
    instruction: Instruction,
    exchange: &Pubkey,
    bank: &Pubkey,
    receiver: &Pubkey,
    owner: &Pubkey,
    tokens: &[Pubkey; 2],
) -> eyre::Result<Option<u32>> {
    let rpc = caller.rpc();
    let before_exchange = rpc.get_account(exchange).await?;
    let before_bank = rpc.get_account(bank).await?;
    let before_receiver = rpc.get_balance(receiver).await?;
    let before_tokens = [
        deployment.get_ata_amount(&tokens[0], owner).await?,
        deployment.get_ata_amount(&tokens[1], owner).await?,
    ];
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
    assert_eq!(
        deployment.get_ata_amount(&tokens[0], owner).await?,
        before_tokens[0]
    );
    assert_eq!(
        deployment.get_ata_amount(&tokens[1], owner).await?,
        before_tokens[1]
    );
    Ok(gmsol_sdk::Error::from(error).anchor_error_code())
}

#[tokio::test]
async fn scub_permissionless_treasury_completion_refunds_recorded_receiver() -> eyre::Result<()> {
    const WINDOW: u32 = 30;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    fund_validator_payer_if_needed().await?;
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
    let initialized_now = prepare_treasury_for_completion_test(deployment, 0).await?;
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
        keeper
            .confirm_gt_exchange_vault(store, &vault, 0, None)
            .send()
            .await?;
        admin
            .treasury_transaction()
            .accounts(vec![
                AccountMeta::new_readonly(admin.payer(), true),
                AccountMeta::new_readonly(*store, false),
                AccountMeta::new_readonly(config, false),
                AccountMeta::new_readonly(vault_config, false),
                AccountMeta::new_readonly(vault, false),
                AccountMeta::new(bank, false),
                AccountMeta::new_readonly(*admin.store_program_id(), false),
            ])
            .args(
                solana_sdk::hash::hash(b"global:confirm_gt_bank_for_test").to_bytes()[..8]
                    .to_vec(),
            )
            .send()
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
    let restore_receiver = async {
        if !initialized_now {
            return Ok::<(), eyre::Report>(());
        }
        TreasuryOps::transfer_receiver(admin, store, &admin.payer())
            .send()
            .await?;
        admin
            .store_transaction()
            .anchor_accounts(accounts::AcceptReceiver {
                next_receiver: admin.payer(),
                store: *store,
            })
            .anchor_args(args::AcceptReceiver {})
            .send()
            .await?;
        Ok::<(), eyre::Report>(())
    }
    .await;
    result?;
    restore_window?;
    restore_receiver?;
    Ok(())
}

#[tokio::test]
async fn scub_permissionless_completion_uses_recorded_bank_balance_and_order() -> eyre::Result<()> {
    const WINDOW: u32 = 30;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    fund_validator_payer_if_needed().await?;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = &deployment.client;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let caller = deployment.user_client(Deployment::DEFAULT_USER)?;
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let fallback_owner = Keypair::new();
    let rent_payer = Keypair::new();
    let tokens = [
        deployment
            .token("WSOL")
            .expect("WSOL token must exist")
            .address,
        deployment
            .token("fBTC")
            .expect("fBTC token must exist")
            .address,
    ];
    let store = &deployment.store;
    let config = admin.find_treasury_config_address(store);
    let treasury_receiver = admin.find_treasury_receiver_address(&config);
    let vault_config = admin.find_treasury_vault_config_address(&config, 0);

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
    let initialized_now =
        prepare_treasury_for_completion_test(deployment, gmsol_store::constants::MARKET_USD_UNIT)
            .await?;

    keeper
        .gt_set_exchange_time_window(store, WINDOW)
        .send()
        .await?;
    let result = async {
        for owner in &owners {
            let client = admin.try_clone_with_payer(shared_signer(Keypair::from_bytes(&owner.to_bytes())?))?;
            client.prepare_user(store)?.send().await?;
            keeper.mint_gt_reward(store, &owner.pubkey(), 2)?.send().await?;
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

            for (name, token, amount) in [("WSOL", tokens[0], 3_u64), ("fBTC", tokens[1], 2)] {
                deployment.mint_or_transfer_to(name, &treasury_receiver, amount).await?;
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
            for owner in &owners {
                let client = admin.try_clone_with_payer(shared_signer(Keypair::from_bytes(&owner.to_bytes())?))?;
                client.request_gt_exchange_v2_with_time_window_index(
                    store, &owner.pubkey(), &owner.pubkey(), &rent_payer.pubkey(), index, WINDOW, 1,
                ).signer(&rent_payer).send().await?;
            }
            fallback_client.request_gt_exchange_with_time_window_index(store, index, WINDOW, 0)
                .send().await?;

            let hook_accounts = |authority| vec![
                AccountMeta::new_readonly(authority, true),
                AccountMeta::new_readonly(*store, false),
                AccountMeta::new_readonly(config, false),
                AccountMeta::new_readonly(vault_config, false),
                AccountMeta::new_readonly(vault, false),
                AccountMeta::new(bank, false),
                AccountMeta::new_readonly(*admin.store_program_id(), false),
            ];
            let hook_data = solana_sdk::hash::hash(b"global:confirm_gt_bank_for_test").to_bytes()[..8].to_vec();
            let first_exchange = keeper.find_gt_exchange_address(&vault, &owners[order[0]].pubkey());
            let first_instruction = caller.complete_gt_exchange_v2(
                store, &first_exchange, Some(&vault_config),
                Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                Some(&vault),
            ).await?.take_instruction().expect("completion instruction");
            assert_completion_rejected_atomically(
                &caller, deployment, first_instruction, &first_exchange, &bank,
                &rent_payer.pubkey(), &owners[order[0]].pubkey(), &tokens,
            ).await?;
            let bank_before_hook = caller.rpc().get_account(&bank).await?;
            assert!(admin.treasury_transaction().accounts(hook_accounts(admin.payer()))
                .args(hook_data.clone()).send().await.is_err());
            assert_eq!(caller.rpc().get_account(&bank).await?, bank_before_hook);

            loop {
                let clock_account = keeper.rpc().get_account(&solana_sdk::sysvar::clock::id()).await?;
                let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
                    .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
                if get_time_window_index(clock.unix_timestamp, i64::from(WINDOW)) > index { break; }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            keeper.confirm_gt_exchange_vault(store, &vault, 3, None).send().await?;
            let bank_before_denial = caller.rpc().get_account(&bank).await?;
            assert!(caller.treasury_transaction().accounts(hook_accounts(caller.payer()))
                .args(hook_data.clone()).send().await.is_err());
            assert_eq!(caller.rpc().get_account(&bank).await?, bank_before_denial);
            admin.treasury_transaction()
                .accounts(hook_accounts(admin.payer()))
                .args(hook_data)
                .send().await?;

            let first_owner = owners[order[0]].pubkey();
            let base = caller.complete_gt_exchange_v2(
                store, &first_exchange, Some(&vault_config),
                Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                Some(&vault),
            ).await?.take_instruction().expect("completion instruction");
            assert_eq!(base.accounts.len(), 17);
            assert_eq!(base.accounts[0].pubkey, first_owner);
            assert_eq!(base.accounts[1].pubkey, rent_payer.pubkey());
            assert!(!base.accounts[0].is_signer);
            assert!(base.accounts[1].is_writable);
            assert_eq!([base.accounts[11].pubkey, base.accounts[12].pubkey], tokens);

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
            noncanonical_target.accounts[15].pubkey = noncanonical.pubkey();
            invalid.push(("noncanonical target", noncanonical_target,
                Some(gmsol_programs::anchor_lang::error::ErrorCode::AccountNotAssociatedTokenAccount.into())));
            let mut wrong_mint = base.clone();
            wrong_mint.accounts[11].pubkey = tokens[1];
            invalid.push(("wrong mint", wrong_mint, Some(CoreError::InvalidArgument.into())));
            let mut wrong_vault = base.clone();
            wrong_vault.accounts[13].pubkey = base.accounts[15].pubkey;
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
                Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
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
                    Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
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
                if step < 2 {
                    let target = anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(&owner, &tokens[1]);
                    admin.store_transaction().pre_instruction(
                        anchor_spl::token::spl_token::instruction::freeze_account(
                            &anchor_spl::token::ID, &target, &tokens[1], &admin.payer(), &[],
                        )?, true,
                    ).send().await?;
                    if step == 1 {
                        let frozen_positive = caller.complete_gt_exchange_v2(
                            store, &exchange, Some(&vault_config),
                            Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                            Some(&vault),
                        ).await?.take_instruction().expect("completion instruction");
                        assert_completion_rejected_atomically(
                            &caller, deployment, frozen_positive, &exchange, &bank,
                            &rent_payer.pubkey(), &owner, &tokens,
                        ).await?;
                        admin.store_transaction().pre_instruction(
                            anchor_spl::token::spl_token::instruction::thaw_account(
                                &anchor_spl::token::ID, &target, &tokens[1], &admin.payer(), &[],
                            )?, true,
                        ).send().await?;
                    }
                }
                let before = [
                    deployment.get_ata_amount(&tokens[0], &owner).await?.unwrap_or(0),
                    deployment.get_ata_amount(&tokens[1], &owner).await?.unwrap_or(0),
                ];
                let duplicate = caller.complete_gt_exchange_v2(
                    store, &exchange, Some(&vault_config),
                    Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                    Some(&vault),
                ).await?.take_instruction().expect("completion instruction");
                caller.complete_gt_exchange_v2(
                    store, &exchange, Some(&vault_config),
                    Some(tokens.iter().map(|token| (*token, anchor_spl::token::ID)).collect()),
                    Some(&vault),
                ).await?.send().await?;
                if step == 0 {
                    let target = anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address(&owner, &tokens[1]);
                    admin.store_transaction().pre_instruction(
                        anchor_spl::token::spl_token::instruction::thaw_account(
                            &anchor_spl::token::ID, &target, &tokens[1], &admin.payer(), &[],
                        )?, true,
                    ).send().await?;
                }
                let payouts = [1, if step == 0 { 0 } else { 1 }];
                for (token_index, token) in tokens.iter().enumerate() {
                    assert_eq!(deployment.get_ata_amount(token, &owner).await?, Some(before[token_index] + payouts[token_index]));
                }
                let recorded = caller.account::<ZeroCopy<gmsol_programs::gmsol_treasury::accounts::GtBank>>(&bank)
                    .await?.expect("confirmed bank must exist").0;
                assert_eq!(recorded.remaining_confirmed_gt_amount, 2 - step as u64);
                for (token, expected) in [(tokens[0], 2 - step as u64), (tokens[1], 2 - step as u64)] {
                    let entry = recorded.balances.data.iter()
                        .find(|entry| entry.key == token.to_bytes())
                        .expect("bank token must be recorded");
                    assert_eq!(entry.value.amount, expected);
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
    let restore_receiver = async {
        if !initialized_now {
            return Ok::<(), eyre::Report>(());
        }
        TreasuryOps::transfer_receiver(admin, store, &admin.payer())
            .send()
            .await?;
        admin
            .store_transaction()
            .anchor_accounts(accounts::AcceptReceiver {
                next_receiver: admin.payer(),
                store: *store,
            })
            .anchor_args(args::AcceptReceiver {})
            .send()
            .await?;
        Ok::<(), eyre::Report>(())
    }
    .await;
    let restore_factor = admin.set_gt_factor(store, 0)?.send().await;
    result?;
    restore_window?;
    restore_receiver?;
    restore_factor?;
    Ok(())
}

#[tokio::test]
async fn scub_close_gt_exchange_checks_receiver_and_controller() -> eyre::Result<()> {
    const SCUB_WINDOW: u32 = 30;

    let _window_guard = GT_EXCHANGE_WINDOW_LOCK.lock().await;
    fund_validator_payer_if_needed().await?;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let owner = deployment.user_client(Deployment::USER_1)?;
    let second_owner = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let owner_address = owner.payer();
    let second_owner_address = second_owner.payer();

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

        let clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
            .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let scub_remaining =
            i64::from(SCUB_WINDOW) - clock.unix_timestamp.rem_euclid(i64::from(SCUB_WINDOW));
        if scub_remaining < 10 {
            tokio::time::sleep(Duration::from_secs((scub_remaining + 1) as u64)).await;
        }
        let clock_account = keeper
            .rpc()
            .get_account(&solana_sdk::sysvar::clock::id())
            .await?;
        let clock: solana_sdk::clock::Clock = solana_sdk::account::from_account(&clock_account)
            .ok_or_else(|| eyre::eyre!("invalid validator clock"))?;
        let index = get_time_window_index(clock.unix_timestamp, i64::from(SCUB_WINDOW));
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
