use libsecp256k1::{recover, Message as SecpMessage, RecoveryId, Signature};
use solana_sdk::{keccak, pubkey::Pubkey};

use super::types::{DecimalsMap, EncodedRecommendation, ValuesMap};

fn pad_utf8<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [0u8; N];
    let bytes = s.as_bytes();
    let len = bytes.len().min(N);
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

fn parse_uuid_16(ref_id: &str) -> crate::Result<[u8; 16]> {
    let v = uuid::Uuid::parse_str(ref_id).map_err(crate::Error::custom)?;
    Ok(*v.as_bytes())
}

fn push_kv(
    msg: &mut Vec<u8>,
    values: &ValuesMap,
    decimals: &DecimalsMap,
    key: &str,
) -> crate::Result<()> {
    let v = *values
        .get(key)
        .ok_or_else(|| crate::Error::custom(format!("missing value for {key}")))?;
    let d = *decimals
        .get(key)
        .ok_or_else(|| crate::Error::custom(format!("missing decimals for {key}")))?;
    msg.extend_from_slice(&v.to_le_bytes());
    msg.push(d);
    Ok(())
}

pub fn build_signed_message(rec: &EncodedRecommendation) -> crate::Result<[u8; 32]> {
    let mut msg = Vec::new();

    msg.extend_from_slice(&pad_utf8::<32>(&rec.parameter_name));

    let market = rec.market_pubkey()?;
    msg.extend_from_slice(&market.to_bytes());

    match rec.parameter_name.as_str() {
        "oiCaps" => {
            push_kv(
                &mut msg,
                &rec.new_values,
                &rec.decimals,
                "oiCaps/maxOpenInterestForLongs/v1",
            )?;
            push_kv(
                &mut msg,
                &rec.new_values,
                &rec.decimals,
                "oiCaps/maxOpenInterestForShorts/v1",
            )?;
        }
        "priceImpact" => {
            push_kv(
                &mut msg,
                &rec.new_values,
                &rec.decimals,
                "priceImpact/negativePositionImpactFactor/v1",
            )?;
            push_kv(
                &mut msg,
                &rec.new_values,
                &rec.decimals,
                "priceImpact/positionImpactExponentFactor/v1",
            )?;
            push_kv(
                &mut msg,
                &rec.new_values,
                &rec.decimals,
                "priceImpact/positivePositionImpactFactor/v1",
            )?;
        }
        _ => return Err(crate::Error::custom("unsupported parameter_name")),
    }

    msg.extend_from_slice(&rec.timestamp.to_le_bytes());

    msg.extend_from_slice(&pad_utf8::<16>(&rec.protocol));

    let ref_id = parse_uuid_16(&rec.reference_id)?;
    msg.extend_from_slice(&ref_id);

    let hash = keccak::hash(&msg);
    Ok(hash.to_bytes())
}

pub fn verify_signature(
    rec: &EncodedRecommendation,
    expected_signer: &Pubkey,
) -> crate::Result<()> {
    let hash = build_signed_message(rec)?;

    let mut sig_bytes = [0u8; 64];
    let sig_str = rec.signature.trim();
    let sig_hex = sig_str
        .strip_prefix("0x")
        .or_else(|| sig_str.strip_prefix("0X"))
        .unwrap_or(sig_str);
    let sig_vec = hex::decode(sig_hex).map_err(crate::Error::custom)?;
    if sig_vec.len() != 64 {
        return Err(crate::Error::custom(
            "invalid signature length; expected 64 bytes",
        ));
    }
    sig_bytes.copy_from_slice(&sig_vec);

    let rid = RecoveryId::parse(rec.recovery_id)
        .map_err(|_| crate::Error::custom("invalid recovery id"))?;

    let msg = SecpMessage::parse(&hash);
    let sig = Signature::parse_standard(&sig_bytes)
        .map_err(|_| crate::Error::custom("invalid signature format"))?;
    let pk = recover(&msg, &sig, &rid)
        .map_err(|_| crate::Error::custom("secp256k1 public key recovery failed"))?;

    let compressed = pk.serialize_compressed();
    let sol_hash = keccak::hash(&compressed);
    let recovered = Pubkey::from(sol_hash.to_bytes());

    if &recovered != expected_signer {
        return Err(crate::Error::custom(
            "signature verification failed: unexpected signer",
        ));
    }
    Ok(())
}
