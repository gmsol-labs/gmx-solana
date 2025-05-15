use base64::{prelude::BASE64_STANDARD, Engine};
use gmsol_programs::anchor_lang::prelude::borsh::BorshDeserialize;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_client::SerializableTransaction};

/// View the return data by simulating the transaction.
pub async fn view<T: BorshDeserialize>(
    client: &RpcClient,
    transaction: &impl SerializableTransaction,
) -> crate::Result<T> {
    let res = client
        .simulate_transaction(transaction)
        .await
        .map_err(crate::Error::unknown)?;
    if let Some(error) = res.value.err {
        return Err(crate::Error::unknown(format!(
            "error={error}, logs={:#?}",
            res.value.logs,
        )));
    }
    let (data, _encoding) = res
        .value
        .return_data
        .ok_or(crate::Error::unknown("missing return data"))?
        .data;
    let decoded = BASE64_STANDARD.decode(data)?;
    let output = T::deserialize_reader(&mut decoded.as_slice())
        .map_err(crate::error::AnchorLangError::from)?;
    Ok(output)
}
