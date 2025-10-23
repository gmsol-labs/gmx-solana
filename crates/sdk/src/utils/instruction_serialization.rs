use std::borrow::Cow;

use base64::{engine::general_purpose::STANDARD, Engine};
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

/// Instruction serialziation format.
#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum InstructionSerialization {
    /// Base58 (Squads).
    Base58,
    #[default]
    /// Base64.
    Base64,
    /// Base58 (Legacy).
    Base58Legacy,
}

/// Serialize an instruction.
pub fn serialize_instruction(
    ix: &Instruction,
    format: InstructionSerialization,
    payer: Option<&Pubkey>,
) -> crate::Result<String> {
    use solana_sdk::message::legacy::Message;

    let message = match format {
        InstructionSerialization::Base58
        | InstructionSerialization::Base64
        | InstructionSerialization::Base58Legacy => {
            let message = Message::new(&[ix.clone()], payer);
            match format {
                InstructionSerialization::Base58 | InstructionSerialization::Base58Legacy => {
                    bs58::encode(message.serialize()).into_string()
                }
                InstructionSerialization::Base64 => STANDARD.encode(message.serialize()),
            }
        }
    };

    Ok(message)
}

/// Serialize message.
pub fn serialize_message(
    message: &solana_sdk::message::VersionedMessage,
    format: InstructionSerialization,
) -> crate::Result<String> {
    let message = match format {
        InstructionSerialization::Base58 => bs58::encode(message.serialize()).into_string(),
        InstructionSerialization::Base64 => STANDARD.encode(message.serialize()),
        InstructionSerialization::Base58Legacy => {
            let message = to_legacy_message(message)?;
            bs58::encode(message.serialize()).into_string()
        }
    };
    Ok(message)
}

/// Convert to legacy message.
pub fn to_legacy_message<'a>(
    message: &'a solana_sdk::message::VersionedMessage,
) -> crate::Result<Cow<'a, solana_sdk::message::legacy::Message>> {
    use solana_sdk::message::{legacy::Message, VersionedMessage};

    match message {
        VersionedMessage::Legacy(message) => Ok(Cow::Borrowed(message)),
        VersionedMessage::V0(message) => {
            if !message.address_table_lookups.is_empty() {
                return Err(crate::Error::custom("a v0 message that includes address table lookups cannot be converted to a legacy message"));
            }
            Ok(Cow::Owned(Message {
                header: message.header,
                account_keys: message.account_keys.clone(),
                recent_blockhash: message.recent_blockhash,
                instructions: message.instructions.clone(),
            }))
        }
    }
}
