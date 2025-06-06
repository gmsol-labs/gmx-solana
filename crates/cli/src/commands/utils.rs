use eyre::OptionExt;
use gmsol_sdk::{
    client::token_map::TokenMap,
    core::token_config::TokenMapAccess,
    programs::{anchor_lang::prelude::Pubkey, gmsol_store::accounts::Market},
    solana_utils::solana_sdk::{signature::Keypair, signer::EncodableKey},
    utils::{market::MarketDecimals, unsigned_amount_to_decimal, Amount, Value},
};
use rand::{rngs::StdRng, SeedableRng};
use rust_decimal::{Decimal, MathematicalOps};

pub(crate) fn get_token_amount_with_token_map(
    amount: &Amount,
    token: &Pubkey,
    token_map: &TokenMap,
) -> eyre::Result<u64> {
    let decimals = token_map
        .get(token)
        .ok_or_eyre("token config not found")?
        .token_decimals;
    Ok(amount.to_u64(decimals)?)
}

pub(crate) fn token_amount(
    amount: &Amount,
    token: Option<&Pubkey>,
    token_map: &TokenMap,
    market: &Market,
    is_long: bool,
) -> eyre::Result<u64> {
    let token = match token {
        Some(token) => token,
        None => {
            if is_long {
                &market.meta.long_token_mint
            } else {
                &market.meta.short_token_mint
            }
        }
    };
    get_token_amount_with_token_map(amount, token, token_map)
}

pub(crate) fn unit_price(
    price: &Value,
    token_map: &TokenMap,
    market: &Market,
) -> eyre::Result<u128> {
    let decimals = MarketDecimals::new(&market.meta.into(), token_map)?;
    let mut price = *price;
    price.0 /= Decimal::TEN.powu(decimals.index_token_decimals.into());

    Ok(price.to_u128()?)
}

/// Price to min output amount.
pub(crate) fn price_to_min_output_amount(
    token_map: &TokenMap,
    token_in: &Pubkey,
    token_in_amount: u64,
    token_out: &Pubkey,
    token_in_to_token_out_price: Value,
) -> Option<u64> {
    let token_in = token_map.get(token_in)?;
    let token_in_amount = unsigned_amount_to_decimal(token_in_amount, token_in.token_decimals());
    let token_out_amount = Amount(token_in_amount.checked_div(token_in_to_token_out_price.0)?);
    let token_out = token_map.get(token_out)?;
    token_out_amount.to_u64(token_out.token_decimals).ok()
}

pub(crate) fn toml_from_file<T>(path: &impl AsRef<std::path::Path>) -> eyre::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    use std::io::Read;

    let mut buffer = String::new();
    std::fs::File::open(path)?.read_to_string(&mut buffer)?;
    Ok(toml::from_str(&buffer)?)
}

#[derive(Debug, clap::Args)]
pub(crate) struct KeypairArgs {
    /// Path to the keypair of the account to use.
    /// If not provided, a new keypair will be generated.
    keypair: Option<std::path::PathBuf>,
    /// Optional random seed to use for keypair generation.
    #[arg(long)]
    seed: Option<u64>,
}

impl KeypairArgs {
    pub(crate) fn to_keypair(&self) -> eyre::Result<Keypair> {
        let keypair = match self.keypair.as_ref() {
            Some(path) => Keypair::read_from_file(path).map_err(|err| eyre::eyre!("{err}"))?,
            None => {
                let mut rng = if let Some(seed) = self.seed {
                    StdRng::seed_from_u64(seed)
                } else {
                    StdRng::from_entropy()
                };
                Keypair::generate(&mut rng)
            }
        };
        Ok(keypair)
    }
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ToggleValue {
    #[arg(long)]
    enable: bool,
    #[arg(long)]
    disable: bool,
}

impl ToggleValue {
    pub(crate) fn is_enable(&self) -> bool {
        debug_assert!(self.enable != self.disable);
        self.enable
    }
}

/// Side.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub(crate) enum Side {
    /// Long.
    Long,
    /// Short.
    Short,
}

impl Side {
    /// Is long side.
    pub(crate) fn is_long(&self) -> bool {
        matches!(self, Self::Long)
    }
}
