use gmsol_evm_proxy::{accounts, instruction, Proxy, ID, PROXY_VERSION};
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, Discriminator};
use gmsol_sdk::{client::ops::SystemProgramOps, pda::find_proxy_address};
use gmsol_solana_utils::signer::shared_signer;
use solana_sdk::{
    instruction::InstructionError,
    signature::{Keypair, Signer},
    system_program,
    transaction::TransactionError,
};

use crate::anchor_test::setup::current_deployment;

#[tokio::test]
async fn proxy_creation_on_validator_preserves_identity_and_rejects_invalid_requests(
) -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let first_payer = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    let second_payer = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    for payer in [first_payer.payer(), second_payer.payer()] {
        deployment
            .client
            .transfer(&payer, 50_000_000)?
            .send()
            .await?;
    }

    let first_address = [0x23; 20];
    let second_address = [0x47; 20];
    let (first_proxy, first_bump) = find_proxy_address(&first_address, &ID);
    let (second_proxy, second_bump) = find_proxy_address(&second_address, &ID);
    assert_ne!(first_proxy, second_proxy);
    assert!(first_payer.rpc().get_account(&first_proxy).await.is_err());

    first_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: first_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: first_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    let first_account = first_payer.rpc().get_account(&first_proxy).await?;
    assert_eq!(first_account.owner, ID);
    assert_eq!(first_account.data.len(), 8 + std::mem::size_of::<Proxy>());
    assert_eq!(&first_account.data[..8], Proxy::DISCRIMINATOR);
    let first_state = Proxy::try_deserialize(&mut first_account.data.as_slice())?;
    assert_eq!(first_state.version, PROXY_VERSION);
    assert_eq!(first_state.bump, first_bump);
    assert_eq!(first_state.evm_address, first_address);
    assert_eq!(first_state.reserved, [0; 256]);

    second_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: first_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        first_account
    );

    let wrong_proxy = first_proxy;
    let wrong_address = second_address;
    let err = second_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: wrong_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: wrong_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a different address cannot reuse an existing proxy PDA");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        first_account
    );
    assert!(second_payer.rpc().get_account(&second_proxy).await.is_err());

    second_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: second_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: second_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let second_account = second_payer.rpc().get_account(&second_proxy).await?;
    let second_state = Proxy::try_deserialize(&mut second_account.data.as_slice())?;
    assert_eq!(second_state.version, PROXY_VERSION);
    assert_eq!(second_state.bump, second_bump);
    assert_eq!(second_state.evm_address, second_address);
    assert_eq!(second_state.reserved, [0; 256]);
    assert_eq!(
        first_payer.rpc().get_account(&first_proxy).await?,
        first_account
    );

    let rent_payer = Keypair::new();
    let empty_account_rent = first_payer
        .rpc()
        .get_minimum_balance_for_rent_exemption(0)
        .await?;
    let proxy_rent = first_payer
        .rpc()
        .get_minimum_balance_for_rent_exemption(8 + std::mem::size_of::<Proxy>())
        .await?;
    assert!(empty_account_rent < proxy_rent);
    deployment
        .client
        .transfer(&rent_payer.pubkey(), empty_account_rent)?
        .send()
        .await?;
    assert_eq!(
        first_payer.rpc().get_balance(&rent_payer.pubkey()).await?,
        empty_account_rent
    );
    let third_address = [0x69; 20];
    let (third_proxy, _) = find_proxy_address(&third_address, &ID);
    let err = first_payer
        .store_transaction()
        .program(ID)
        .signer(&rent_payer)
        .anchor_args(instruction::CreateProxy {
            evm_address: third_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: rent_payer.pubkey(),
            proxy: third_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a signer funded for only an empty account cannot fund proxy rent");
    let transaction_error = match &err {
        gmsol_solana_utils::Error::Client(client_error) => client_error.get_transaction_error(),
        _ => panic!("unexpected insufficient-rent error: {err:?}"),
    };
    assert!(
        matches!(
            transaction_error,
            Some(TransactionError::InstructionError(
                _,
                InstructionError::Custom(1)
            ))
        ),
        "unexpected insufficient-rent error: {err:?}"
    );
    assert_eq!(
        first_payer.rpc().get_balance(&rent_payer.pubkey()).await?,
        empty_account_rent
    );
    assert!(first_payer.rpc().get_account(&third_proxy).await.is_err());
    Ok(())
}
