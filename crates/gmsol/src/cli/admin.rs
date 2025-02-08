use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::{
    store::{roles::RolesOps, store_ops::StoreOps},
    utils::instruction::InstructionSerialization,
};
use gmsol_solana_utils::bundle_builder::BundleBuilder;
use gmsol_store::states::RoleKey;
use gmsol_timelock::roles as timelock_roles;
use gmsol_treasury::roles as treasury_roles;
use indexmap::IndexSet;
use solana_sdk::signature::Keypair;

use crate::{GMSOLClient, InstructionBufferCtx};

#[derive(clap::Args)]
pub(super) struct AdminArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Create a new data store.
    CreateStore {},
    /// Transfer store authority.
    TransferStoreAuthority {
        #[arg(long)]
        new_authority: Pubkey,
        #[arg(long)]
        confirm: bool,
    },
    /// Accept store authority.
    AcceptStoreAuthority,
    /// Transfer receiver.
    TransferReceiver {
        new_receiver: Pubkey,
        #[arg(long)]
        confirm: bool,
    },
    /// Enable a role.
    EnableRole { role: String },
    /// Disable a role.
    DisableRole { role: String },
    /// Grant a role to a user.
    GrantRole {
        /// User.
        authority: Pubkey,
        /// Role.
        role: String,
    },
    /// Revoke a role from the user.
    RevokeRole {
        /// User.
        authority: Pubkey,
        /// Role.
        role: String,
    },
    /// Initialize roles.
    InitRoles(Box<InitializeRoles>),
}

impl AdminArgs {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store_key: &str,
        ctx: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
    ) -> gmsol::Result<()> {
        let store = client.find_store_address(store_key);
        match &self.command {
            Command::InitRoles(args) => {
                crate::utils::instruction_buffer_not_supported(ctx)?;
                args.run(client, store_key, serialize_only).await?
            }
            Command::CreateStore {} => {
                tracing::info!(
                    "Initialize store with key={store_key}, address={store}, admin={}",
                    client.payer()
                );
                crate::utils::send_or_serialize_transaction(
                    &store,
                    client.initialize_store::<Keypair>(store_key, None, None, None),
                    ctx,
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!("initialized a new data store at tx {signature}");
                        println!("{store}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::TransferStoreAuthority {
                new_authority,
                confirm,
            } => {
                let rpc = client.transfer_store_authority(&store, new_authority);
                if *confirm || serialize_only.is_some() {
                    crate::utils::send_or_serialize_transaction(
                        &store,
                        rpc,
                        ctx,
                        serialize_only,
                        skip_preflight,
                        |signature| {
                            tracing::info!(
                            "transferred store authority to `{new_authority}` at tx {signature}"
                        );
                            Ok(())
                        },
                    )
                    .await?;
                } else {
                    let transaction = rpc.signed_transaction_with_options(true, None).await?;
                    let response = client
                        .store_program()
                        .rpc()
                        .simulate_transaction(&transaction)
                        .await
                        .map_err(anchor_client::ClientError::from)?;
                    tracing::info!("Simulation result: {:#?}", response.value);
                    if response.value.err.is_none() {
                        tracing::info!("The simulation was successful, but this operation is very dangerous. If you are sure you want to proceed, please reauthorize the command with `--confirm` flag");
                    }
                }
            }
            Command::AcceptStoreAuthority => {
                crate::utils::send_or_serialize_transaction(
                    &store,
                    client.accept_store_authority(&store),
                    ctx,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        tracing::info!("accepted store authority at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::TransferReceiver {
                new_receiver,
                confirm,
            } => {
                let rpc = client.transfer_receiver(&store, new_receiver);
                if *confirm || serialize_only.is_some() {
                    crate::utils::send_or_serialize_transaction(
                        &store,
                        rpc,
                        ctx,
                        serialize_only,
                        skip_preflight,
                        |signature| {
                            tracing::info!(
                                "transferred receiver authority to `{new_receiver}` at tx {signature}"
                            );
                            Ok(())
                        },
                    )
                    .await?;
                } else {
                    let transaction = rpc.signed_transaction_with_options(true, None).await?;
                    let response = client
                        .store_program()
                        .rpc()
                        .simulate_transaction(&transaction)
                        .await
                        .map_err(anchor_client::ClientError::from)?;
                    tracing::info!("Simulation result: {:#?}", response.value);
                    if response.value.err.is_none() {
                        tracing::info!("The simulation was successful, but this operation is very dangerous. If you are sure you want to proceed, please reauthorize the command with `--confirm` flag");
                    }
                }
            }
            Command::EnableRole { role } => {
                crate::utils::send_or_serialize_transaction(
                    &store,
                    client.enable_role(&store, role),
                    ctx,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        tracing::info!("enabled role `{role}` at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::DisableRole { role } => {
                crate::utils::send_or_serialize_transaction(
                    &store,
                    client.disable_role(&store, role),
                    ctx,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        tracing::info!("disabled role `{role}` at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::GrantRole { role, authority } => {
                crate::utils::send_or_serialize_transaction(
                    &store,
                    client.grant_role(&store, authority, role),
                    ctx,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        tracing::info!("granted a role for user {authority} at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::RevokeRole { role, authority } => {
                crate::utils::send_or_serialize_transaction(
                    &store,
                    client.revoke_role(&store, authority, role),
                    ctx,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        tracing::info!("revoked a role for user {authority} at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
        }
        Ok(())
    }
}

#[derive(clap::Args)]
struct InitializeRoles {
    #[arg(long)]
    init_store: bool,
    #[arg(long)]
    treasury_admin: Pubkey,
    #[arg(long)]
    treasury_withdrawer: Pubkey,
    #[arg(long)]
    treasury_keeper: Pubkey,
    #[arg(long)]
    timelock_admin: Pubkey,
    #[arg(long)]
    market_keeper: Pubkey,
    #[arg(long)]
    order_keeper: Vec<Pubkey>,
    #[arg(long)]
    allow_multiple_transactions: bool,
    #[arg(long)]
    skip_preflight: bool,
    #[arg(long)]
    max_transaction_size: Option<usize>,
}

impl InitializeRoles {
    async fn run(
        &self,
        client: &GMSOLClient,
        store_key: &str,
        serialize_only: Option<InstructionSerialization>,
    ) -> gmsol::Result<()> {
        let store = client.find_store_address(store_key);

        let mut builder = BundleBuilder::from_rpc_client_with_options(
            client.store_program().rpc(),
            !self.allow_multiple_transactions,
            self.max_transaction_size,
            None,
        );

        if self.init_store {
            // Insert initialize store instruction.
            builder.try_push(client.initialize_store::<Keypair>(store_key, None, None, None))?;
        }

        let treasury_global_config = client.find_treasury_config_address(&store);

        builder
            .push_many(
                [
                    RoleKey::RESTART_ADMIN,
                    RoleKey::GT_CONTROLLER,
                    RoleKey::MARKET_KEEPER,
                    RoleKey::ORDER_KEEPER,
                    RoleKey::FEATURE_KEEPER,
                    RoleKey::CONFIG_KEEPER,
                    RoleKey::ORACLE_CONTROLLER,
                    treasury_roles::TREASURY_OWNER,
                    treasury_roles::TREASURY_ADMIN,
                    treasury_roles::TREASURY_WITHDRAWER,
                    treasury_roles::TREASURY_KEEPER,
                    timelock_roles::TIMELOCK_ADMIN,
                    timelock_roles::TIMELOCK_KEEPER,
                    timelock_roles::TIMELOCKED_ADMIN,
                ]
                .iter()
                .map(|role| client.enable_role(&store, role)),
                false,
            )?
            .try_push(client.grant_role(&store, &self.market_keeper, RoleKey::MARKET_KEEPER))?
            .try_push(client.grant_role(
                &store,
                &treasury_global_config,
                RoleKey::ORACLE_CONTROLLER,
            ))?
            .try_push(client.grant_role(&store, &treasury_global_config, RoleKey::GT_CONTROLLER))?
            .try_push(client.grant_role(
                &store,
                &self.treasury_admin,
                treasury_roles::TREASURY_ADMIN,
            ))?
            .try_push(client.grant_role(
                &store,
                &self.treasury_withdrawer,
                treasury_roles::TREASURY_WITHDRAWER,
            ))?
            .try_push(client.grant_role(
                &store,
                &self.treasury_keeper,
                treasury_roles::TREASURY_KEEPER,
            ))?
            .try_push(client.grant_role(
                &store,
                &self.timelock_admin,
                timelock_roles::TIMELOCK_ADMIN,
            ))?
            .try_push(client.grant_role(
                &store,
                &self.timelock_admin,
                timelock_roles::TIMELOCK_KEEPER,
            ))?
            .try_push(client.grant_role(
                &store,
                &self.timelock_admin,
                timelock_roles::TIMELOCKED_ADMIN,
            ))?;

        for keeper in self.unique_order_keepers() {
            builder.try_push(client.grant_role(&store, keeper, RoleKey::ORDER_KEEPER))?;
        }

        crate::utils::send_or_serialize_bundle(
            &store,
            builder,
            None,
            serialize_only,
            self.skip_preflight,
            |signatures, error| {
                println!("{signatures:#?}");
                match error {
                    None => Ok(()),
                    Some(err) => Err(err),
                }
            },
        )
        .await?;
        Ok(())
    }

    fn unique_order_keepers(&self) -> impl IntoIterator<Item = &Pubkey> {
        self.order_keeper.iter().collect::<IndexSet<_>>()
    }
}
