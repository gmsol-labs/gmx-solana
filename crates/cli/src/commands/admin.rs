use std::collections::BTreeMap;

use gmsol_sdk::ops::StoreOps;

use crate::config::DisplayOptions;

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
    /// Initialize callback authority.
    InitCallbackAuthority,
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
                let store = client.store(store).await?;
                let role_store = &store.role;
                let roles = std::iter::once(Ok(ADMIN))
                    .chain(role_store.roles().map(|res| res.map_err(eyre::Error::from)))
                    .collect::<eyre::Result<Vec<_>>>()?;
                let mut members = role_store
                    .members()
                    .map(|member| {
                        let roles = roles
                            .iter()
                            .filter_map(|role| {
                                if *role == ADMIN {
                                    if store.authority == member {
                                        Some(Ok(ADMIN))
                                    } else {
                                        None
                                    }
                                } else {
                                    match role_store.has_role(&member, role) {
                                        Ok(true) => Some(Ok(*role)),
                                        Ok(false) => None,
                                        Err(err) => Some(Err(err)),
                                    }
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()?
                            .join("|");
                        Ok((
                            member,
                            serde_json::json!({
                                "roles": roles,
                            }),
                        ))
                    })
                    .collect::<eyre::Result<BTreeMap<_, _>>>()?;
                members.entry(store.authority).or_insert_with(|| {
                    serde_json::json!({
                    "roles": ADMIN,
                    })
                });
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
            Command::InitCallbackAuthority => client
                .initialize_callback_authority()
                .into_bundle_with_options(options)?,
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
