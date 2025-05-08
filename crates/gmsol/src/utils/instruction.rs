use anchor_client::solana_sdk::instruction::Instruction;
use base64::{engine::general_purpose::STANDARD, Engine};
use solana_sdk::pubkey::Pubkey;
// use spl_governance::state::proposal_transaction::InstructionData;

/// Instruction serialziation format.
#[derive(Default, Clone, Copy)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
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
        // InstructionSerialization::Gov => {
        //     let data = InstructionData::from(ix.clone()).try_to_vec()?;
        //     STANDARD.encode(data)
        // }
        InstructionSerialization::Base58 | InstructionSerialization::Base64 => {
            let message = Message::new(&[ix.clone()], payer);
            match format {
                InstructionSerialization::Base58 => bs58::encode(message.serialize()).into_string(),
                InstructionSerialization::Base64 => STANDARD.encode(message.serialize()),
                // _ => {
                //     unreachable!()
                // }
            }
        }
    };

    Ok(message)
}
