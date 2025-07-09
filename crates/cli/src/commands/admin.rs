use std::collections::BTreeMap;

use gmsol_sdk::{
    ops::{RoleOps, StoreOps},
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::{
        bundle_builder::{BundleBuilder, BundleOptions},
        signer::LocalSignerRef,
        solana_sdk::signature::NullSigner,
        transaction_builder::default_before_sign,
    },
};
use indexmap::IndexSet;

use crate::config::DisplayOptions;

use super::CommandClient;

/// Administrative commands.
#[derive(Debug, clap::Args)]
pub struct Admin {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Display member table.
    Members,
    /// Display role table.
    Roles,
    /// Initialize a new store.
    InitStore { key: String },
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
        #[arg(long)]
        role: String,
    },
    /// Revoke a role from the user.
    RevokeRole {
        /// User.
        authority: Pubkey,
        /// Role.
        #[arg(long)]
        role: String,
    },
    /// Initialize roles.
    InitRoles(Box<InitializeRoles>),
    /// Initialize callback authority.
    InitCallbackAuthority,
    /// Update last restarted slot.
    UpdateLastRestartedSlot,
}

impl super::Command for Admin {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();
        let output = ctx.config().output();

        let bundle = match &self.command {
            Command::Members => {
                const ADMIN: &str = "ADMIN";
                const RECEIVER: &str = "RECEIVER";
                const HOLDING: &str = "HOLDING";
                let store = client.store(store).await?;
                let role_store = &store.role;
                let roles = std::iter::once(Ok(ADMIN))
                    .chain(Some(Ok(RECEIVER)))
                    .chain(Some(Ok(HOLDING)))
                    .chain(role_store.roles().map(|res| res.map_err(eyre::Error::from)))
                    .collect::<eyre::Result<Vec<_>>>()?;
                let members = role_store
                    .members()
                    .chain(Some(store.authority))
                    .chain(Some(store.treasury.receiver))
                    .chain(Some(store.address.holding))
                    .collect::<IndexSet<_>>();
                let members = members
                    .into_iter()
                    .map(|member| {
                        let roles = roles
                            .iter()
                            .filter_map(|role| {
                                if *role == ADMIN {
                                    if store.authority == member {
                                        Some(ADMIN)
                                    } else {
                                        None
                                    }
                                } else if *role == RECEIVER {
                                    if store.treasury.receiver == member {
                                        Some(RECEIVER)
                                    } else {
                                        None
                                    }
                                } else if *role == HOLDING {
                                    if store.address.holding == member {
                                        Some(HOLDING)
                                    } else {
                                        None
                                    }
                                } else {
                                    match role_store.has_role(&member, role) {
                                        Ok(true) => Some(*role),
                                        _ => None,
                                    }
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("|");
                        Ok((
                            member,
                            serde_json::json!({
                                "roles": roles,
                            }),
                        ))
                    })
                    .collect::<eyre::Result<BTreeMap<_, _>>>()?;

                println!(
                    "{}",
                    output.display_keyed_accounts(
                        members,
                        DisplayOptions::table_projection([
                            ("pubkey", "Member"),
                            ("roles", "Roles")
                        ])
                    )?
                );
                return Ok(());
            }
            Command::Roles => {
                let store = client.store(store).await?;
                let role_store = &store.role;
                let roles = role_store
                    .roles()
                    .enumerate()
                    .map(|(idx, res)| {
                        res.map(|name| {
                            serde_json::json!({
                                "index": idx,
                                "role": name,
                            })
                        })
                        .map_err(eyre::Error::from)
                    })
                    .collect::<eyre::Result<Vec<_>>>()?;
                println!(
                    "{}",
                    output.display_many(
                        roles,
                        DisplayOptions::table_projection([("index", "Index"), ("role", "Role")])
                    )?
                );
                return Ok(());
            }
            Command::InitStore { key } => {
                let store_key = key;
                let store = client.find_store_address(store_key);
                let authority = client.payer();
                tracing::info!(
                    "Initialize store with key={store_key}, address={store}, admin={}",
                    authority
                );
                client
                    .initialize_store::<NullSigner>(store_key, None, None, None)
                    .into_bundle_with_options(options)?
            }
            Command::TransferStoreAuthority {
                new_authority,
                confirm,
            } => {
                let rpc = client.transfer_store_authority(store, new_authority);
                if *confirm || client.serialize_only.is_some() {
                    rpc.into_bundle_with_options(options)?
                } else {
                    let transaction = rpc
                        .signed_transaction_with_options(true, None, None, default_before_sign)
                        .await?;
                    let response = client
                        .store_program()
                        .rpc()
                        .simulate_transaction(&transaction)
                        .await?;
                    println!("Simulation result: {:#?}", response.value);
                    if response.value.err.is_none() {
                        println!("The simulation was successful, but this operation is very dangerous. If you are sure you want to proceed, please reauthorize the command with `--confirm` flag");
                    }
                    return Ok(());
                }
            }
            Command::AcceptStoreAuthority => client
                .accept_store_authority(store)
                .into_bundle_with_options(options)?,
            Command::TransferReceiver {
                new_receiver,
                confirm,
            } => {
                let rpc = client.transfer_receiver(store, new_receiver);
                if *confirm || client.serialize_only.is_some() {
                    rpc.into_bundle_with_options(options)?
                } else {
                    let transaction = rpc
                        .signed_transaction_with_options(true, None, None, default_before_sign)
                        .await?;
                    let response = client
                        .store_program()
                        .rpc()
                        .simulate_transaction(&transaction)
                        .await?;
                    println!("Simulation result: {:#?}", response.value);
                    if response.value.err.is_none() {
                        println!("The simulation was successful, but this operation is very dangerous. If you are sure you want to proceed, please reauthorize the command with `--confirm` flag");
                    }
                    return Ok(());
                }
            }
            Command::EnableRole { role } => client
                .enable_role(store, role)
                .into_bundle_with_options(options)?,
            Command::DisableRole { role } => client
                .disable_role(store, role)
                .into_bundle_with_options(options)?,
            Command::GrantRole { authority, role } => client
                .grant_role(store, authority, role)
                .into_bundle_with_options(options)?,
            Command::RevokeRole { authority, role } => client
                .revoke_role(store, authority, role)
                .into_bundle_with_options(options)?,
            Command::InitRoles(args) => args.to_bundle(client, options)?,
            Command::InitCallbackAuthority => client
                .initialize_callback_authority()
                .into_bundle_with_options(options)?,
            Command::UpdateLastRestartedSlot => client.update_last_restarted_slot(store).into_bundle_with_options(options)?,
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}

#[derive(Debug, clap::Args)]
struct InitializeRoles {
    #[arg(long)]
    init_store: Option<String>,
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
}

impl InitializeRoles {
    fn to_bundle<'a>(
        &self,
        client: &'a CommandClient,
        options: BundleOptions,
    ) -> eyre::Result<BundleBuilder<'a, LocalSignerRef>> {
        use gmsol_sdk::{core::role::RoleKey, programs::constants::roles};

        let mut builder = client.bundle_with_options(options);

        let store = if let Some(key) = self.init_store.as_ref() {
            builder.push(client.initialize_store::<NullSigner>(key, None, None, None))?;
            client.find_store_address(key)
        } else {
            client.store
        };

        let treasury_global_config = client.find_treasury_config_address(&store);

        builder
            .push_many(
                [
                    RoleKey::RESTART_ADMIN,
                    RoleKey::GT_CONTROLLER,
                    RoleKey::MARKET_KEEPER,
                    RoleKey::ORDER_KEEPER,
                    RoleKey::PRICE_KEEPER,
                    RoleKey::FEATURE_KEEPER,
                    RoleKey::CONFIG_KEEPER,
                    RoleKey::ORACLE_CONTROLLER,
                    roles::TREASURY_OWNER,
                    roles::TREASURY_ADMIN,
                    roles::TREASURY_WITHDRAWER,
                    roles::TREASURY_KEEPER,
                    roles::TIMELOCK_ADMIN,
                    roles::TIMELOCK_KEEPER,
                    roles::TIMELOCKED_ADMIN,
                ]
                .iter()
                .map(|role| client.enable_role(&store, role)),
                false,
            )?
            .push(client.grant_role(&store, &self.market_keeper, RoleKey::MARKET_KEEPER))?
            .push(client.grant_role(&store, &treasury_global_config, RoleKey::ORACLE_CONTROLLER))?
            .push(client.grant_role(&store, &treasury_global_config, RoleKey::GT_CONTROLLER))?
            .push(client.grant_role(&store, &self.treasury_admin, roles::TREASURY_ADMIN))?
            .push(client.grant_role(
                &store,
                &self.treasury_withdrawer,
                roles::TREASURY_WITHDRAWER,
            ))?
            .push(client.grant_role(&store, &self.treasury_keeper, roles::TREASURY_KEEPER))?
            .push(client.grant_role(&store, &self.timelock_admin, roles::TIMELOCK_ADMIN))?
            .push(client.grant_role(&store, &self.timelock_admin, roles::TIMELOCK_KEEPER))?
            .push(client.grant_role(&store, &self.timelock_admin, roles::TIMELOCKED_ADMIN))?;

        for keeper in self.unique_order_keepers() {
            builder
                .push(client.grant_role(&store, keeper, RoleKey::ORDER_KEEPER))?
                .push(client.grant_role(&store, keeper, RoleKey::PRICE_KEEPER))?;
        }

        Ok(builder)
    }

    fn unique_order_keepers(&self) -> impl IntoIterator<Item = &Pubkey> {
        self.order_keeper.iter().collect::<IndexSet<_>>()
    }
}
