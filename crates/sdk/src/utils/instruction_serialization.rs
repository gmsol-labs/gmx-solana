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
}

/// Serialize an instruction.
pub fn serialize_instruction(
    ix: &Instruction,
    format: InstructionSerialization,
    payer: Option<&Pubkey>,
) -> crate::Result<String> {
    use solana_sdk::message::legacy::Message;

    let message = match format {
        InstructionSerialization::Base58 | InstructionSerialization::Base64 => {
            let message = Message::new(&[ix.clone()], payer);
            match format {
                InstructionSerialization::Base58 => bs58::encode(message.serialize()).into_string(),
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
    };
    Ok(message)
}
