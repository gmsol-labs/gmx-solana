use std::path::PathBuf;

use gmsol_sdk::solana_utils::{
    signer::LocalSignerRef,
    solana_sdk::signature::{read_keypair_file, Keypair},
};
use url::Url;

#[cfg(feature = "remote-wallet")]
use solana_remote_wallet::remote_wallet::RemoteWalletManager;

/// Parse url or path.
fn parse_url_or_path(source: &str) -> eyre::Result<Url> {
    let url = match Url::parse(source) {
        Ok(url) => url,
        Err(_) => {
            let path = shellexpand::tilde(source);
            let path: PathBuf = path.parse()?;
            let path = std::fs::canonicalize(path)?;
            Url::from_file_path(&path).expect("must be valid file path")
        }
    };

    Ok(url)
}

/// Load keypair.
pub fn load_keypair(source: &str) -> eyre::Result<Keypair> {
    let url = parse_url_or_path(source)?;

    match url.scheme() {
        "file" => read_keypair_file(url.path()).map_err(|err| eyre::eyre!("{err}")),
        other => {
            eyre::bail!("{other} scheme is not support");
        }
    }
}

/// Load signer from url.
pub fn signer_from_source(
    source: &str,
    #[cfg(feature = "remote-wallet")] confirm_key: bool,
    #[cfg(feature = "remote-wallet")] keypair_name: &str,
    #[cfg(feature = "remote-wallet")] wallet_manager: Option<
        &mut Option<std::rc::Rc<RemoteWalletManager>>,
    >,
) -> eyre::Result<LocalSignerRef> {
    use gmsol_sdk::solana_utils::signer::local_signer;

    #[cfg(feature = "remote-wallet")]
    use solana_remote_wallet::{
        locator::Locator, remote_keypair::generate_remote_keypair,
        remote_wallet::maybe_wallet_manager,
    };

    #[cfg(feature = "remote-wallet")]
    use std::collections::HashMap;

    #[cfg(feature = "remote-wallet")]
    use gmsol_sdk::solana_utils::solana_sdk::derivation_path::DerivationPath;

    #[cfg(feature = "remote-wallet")]
    use eyre::OptionExt;

    #[cfg(feature = "remote-wallet")]
    const QUERY_KEY: &str = "key";

    let url = parse_url_or_path(source)?;

    match url.scheme() {
        "file" => {
            let keypair = read_keypair_file(url.path()).map_err(|err| eyre::eyre!("{err}"))?;
            Ok(local_signer(keypair))
        }
        #[cfg(feature = "remote-wallet")]
        "usb" => {
            let Some(wallet_manager) = wallet_manager else {
                eyre::bail!("remote wallet manager is required");
            };
            let manufacturer = url.host_str().ok_or_eyre("missing manufacturer")?;
            let path = url.path();
            let path = path.strip_prefix('/').unwrap_or(path);
            let pubkey = (!path.is_empty()).then_some(path);
            let locator = Locator::new_from_parts(manufacturer, pubkey)?;
            let query = url.query_pairs().collect::<HashMap<_, _>>();
            if query.len() > 1 {
                eyre::bail!("invalid query string, extra fields not supported");
            }
            let derivation_path = query
                .get(QUERY_KEY)
                .map(|value| DerivationPath::from_key_str(value))
                .transpose()?;
            if wallet_manager.is_none() {
                *wallet_manager = maybe_wallet_manager()?;
            }
            let wallet_manager = wallet_manager.as_ref().ok_or_eyre("no device found")?;
            let keypair = generate_remote_keypair(
                locator,
                derivation_path.unwrap_or_default(),
                wallet_manager,
                confirm_key,
                keypair_name,
            )?;
            Ok(local_signer(keypair))
        }
        scheme => Err(eyre::eyre!("unsupported scheme: {scheme}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url_or_path() -> eyre::Result<()> {
        let path = "~/.config/solana/id.json";
        assert!(parse_url_or_path(path).is_ok());
        Ok(())
    }
}
