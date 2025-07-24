use std::{borrow::Cow, env};

use futures_util::TryStreamExt;
use gmsol_sdk::{
    decode::{
        decoder::{
            solana_decoder::solana_transaction_status::{UiInstruction, UiTransactionEncoding},
            TransactionDecoder,
        },
        Decode, DecodeError, Decoder, TransactionAccess, Visitor,
    },
    programs::{
        anchor_lang::{prelude::Pubkey, Discriminator},
        gmsol_store::client::args,
    },
    solana_utils::{
        solana_client::rpc_config::RpcTransactionConfig,
        solana_sdk::{bs58, signature::Keypair},
    },
    Client,
};

#[derive(Debug)]
struct CreatedOrders {
    slot: u64,
    pubkeys: Vec<Pubkey>,
}

impl Decode for CreatedOrders {
    fn decode<D: Decoder>(decoder: D) -> Result<Self, DecodeError> {
        struct CreateOrderVisitor;

        impl Visitor for CreateOrderVisitor {
            type Value = CreatedOrders;

            fn visit_transaction(
                self,
                txn: impl TransactionAccess,
            ) -> Result<Self::Value, DecodeError> {
                let slot = txn.slot()?;
                let ixs = (0..txn.num_instructions())
                    .map(|idx| txn.instruction(idx).expect("must exist"))
                    .map(|ix| (ix.program_id_index, Cow::Borrowed(&ix.data), &ix.accounts));
                let meta = txn
                    .transaction_status_meta()
                    .ok_or_else(|| DecodeError::custom("missing meta"))?;
                let iixs = meta.inner_instructions.as_ref().map(|iixs| iixs);
                let iixs = iixs
                    .iter()
                    .copied()
                    .flatten()
                    .flat_map(|iix| iix.instructions.iter())
                    .map(|iix| {
                        let UiInstruction::Compiled(iix) = iix else {
                            return Err(DecodeError::custom("invalid status meta"));
                        };
                        let data = bs58::decode(&iix.data)
                            .into_vec()
                            .map_err(DecodeError::custom)?;
                        Ok((iix.program_id_index, Cow::Owned(data), &iix.accounts))
                    });
                let mut pubkeys = Vec::default();
                for ix in ixs.map(Ok).chain(iixs) {
                    let (program_id_idx, data, accounts) = ix?;
                    let program_id = txn
                        .account_meta(program_id_idx as usize)?
                        .ok_or_else(|| DecodeError::custom("invalid transaction"))?;
                    if program_id.pubkey != gmsol_sdk::programs::gmsol_store::ID {
                        continue;
                    }

                    if data.len() < 8 {
                        return Err(DecodeError::custom("invalid discriminator"));
                    }

                    let discriminator = &data[..8];
                    let order_idx = match discriminator {
                        args::CreateOrder::DISCRIMINATOR => 5,
                        args::CreateOrderV2::DISCRIMINATOR => 5,
                        _ => {
                            continue;
                        }
                    };
                    let order_idx = accounts
                        .get(order_idx)
                        .ok_or_else(|| DecodeError::custom("invalid transaction"))?;
                    let order = txn
                        .account_meta(*order_idx as usize)?
                        .ok_or_else(|| DecodeError::custom("invalid transaction"))?;
                    pubkeys.push(order.pubkey);
                }
                Ok(CreatedOrders { slot, pubkeys })
            }
        }

        decoder.decode_transaction(CreateOrderVisitor)
    }
}

#[tokio::main]
async fn main() -> gmsol_sdk::Result<()> {
    use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(
                "decode_transaction=info"
                    .parse()
                    .map_err(gmsol_sdk::Error::custom)?,
            ),
        )
        .with_span_events(FmtSpan::FULL)
        .init();

    let cluster = env::var("CLUSTER")
        .unwrap_or_else(|_| "devnet".to_string())
        .parse()?;
    let payer = Keypair::new();

    let client = Client::new(cluster, &payer)?;

    // Passing an empty string returns the default store address.
    let store = client.find_store_address("");

    let pub_sub = client.pub_sub().await?;

    let mut stream = pub_sub.logs_subscribe(&store, None).await?;

    while let Some(resp) = stream.try_next().await? {
        let txn = resp.into_value();
        if txn.err.is_some() {
            continue;
        }
        let signature = txn.signature.parse().map_err(gmsol_sdk::Error::custom)?;
        let txn = client
            .rpc()
            .get_transaction_with_config(
                &signature,
                RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::Base58),
                    commitment: None,
                    max_supported_transaction_version: Some(0),
                },
            )
            .await
            .map_err(gmsol_sdk::Error::custom)?;
        let decoder = TransactionDecoder::new(txn.slot, signature, &txn.transaction);
        let created_orders = CreatedOrders::decode(decoder)?;

        println!(
            "{signature}: slot={}, orders={:?}",
            created_orders.slot, created_orders.pubkeys
        );
    }
    Ok(())
}
