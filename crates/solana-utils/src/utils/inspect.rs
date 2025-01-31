use solana_sdk::message::VersionedMessage;

use crate::cluster::Cluster;

/// Generate inspector url or encoded transaction message for the given message.
pub fn inspect_transaction(
    message: &VersionedMessage,
    cluster: Option<&Cluster>,
    raw: bool,
) -> String {
    use base64::{prelude::BASE64_STANDARD, Engine};
    use url::form_urlencoded;

    let message = BASE64_STANDARD.encode(message.serialize());

    if raw {
        message
    } else {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer.append_pair("message", &message);

        let cluster = cluster.cloned().unwrap_or(Cluster::Mainnet);
        match cluster {
            Cluster::Localnet => {
                serializer
                    .append_pair("cluster", "custom")
                    .append_pair("customUrl", "http://localhost:8899");
            }
            Cluster::Custom(url, _) => {
                serializer
                    .append_pair("cluster", "custom")
                    .append_pair("customUrl", &url);
            }
            _ => {
                serializer.append_pair("cluster", &cluster.to_string());
            }
        }

        format!(
            "https://explorer.solana.com/tx/inspector?{}",
            serializer.finish()
        )
    }
}
