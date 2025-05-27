use std::time::Duration;

use eyre::OptionExt;
use gmsol_sdk::{
    ops::competition::{CompetitionOps, CompetitionParams},
    programs::anchor_lang::prelude::Pubkey,
    utils::UsdValue,
};
use time::OffsetDateTime;

/// Commands for competition management.
#[derive(Debug, clap::Args)]
pub struct Competition {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Initialize a competition.
    Init {
        #[arg(long, value_parser = parse_datetime)]
        start: OffsetDateTime,
        #[arg(long, value_parser = parse_datetime)]
        end: OffsetDateTime,
        #[arg(long)]
        volume_threshold: UsdValue,
        #[arg(long, value_parser = humantime::parse_duration)]
        time_extension: Duration,
        #[arg(long, value_parser = humantime::parse_duration)]
        max_extension: Duration,
    },
    /// Fetch a competition.
    Get { address: Pubkey },
}

fn parse_datetime(s: &str) -> Result<OffsetDateTime, time::error::Parse> {
    use time::format_description::well_known::Rfc3339;

    OffsetDateTime::parse(s, &Rfc3339)
}

impl super::Command for Competition {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let options = ctx.bundle_options();
        let bundle = match &self.command {
            Command::Init {
                start,
                end,
                volume_threshold,
                time_extension,
                max_extension,
            } => {
                let (tx, competition) = client
                    .initialize_competition(
                        &CompetitionParams::builder()
                            .start_time(start.unix_timestamp())
                            .end_time(end.unix_timestamp())
                            .volume_threshold(volume_threshold.to_u128()?)
                            .time_extension(time_extension.as_secs().try_into()?)
                            .max_extension(max_extension.as_secs().try_into()?)
                            .build(),
                    )
                    .swap_output(());

                println!("{competition}");
                tx.into_bundle_with_options(options)?
            }
            Command::Get { address } => {
                let competition = client
                    .account::<gmsol_sdk::programs::gmsol_competition::accounts::Competition>(
                        address,
                    )
                    .await?
                    .ok_or_eyre("competition not found")?;
                println!("{competition:#?}");
                return Ok(());
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
