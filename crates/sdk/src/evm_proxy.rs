//! EVM proxy intent data and typed log events.

use anchor_lang::Discriminator;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use base64::Engine;
use gmsol_programs::gmsol_evm_proxy::events;
use gmsol_programs::{
    anchor_lang::{InstructionData, ToAccountMetas},
    gmsol_evm_proxy::client::{accounts, args},
};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Signed fields of one Withdraw intent. The pair is explicit, including generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WithdrawIntent {
    pub evm_address: [u8; 20],
    pub chain_id: [u8; 32],
    pub mint: Pubkey,
    pub amount: u64,
    pub decimals: u8,
    pub recipient: Pubkey,
    pub generation: u64,
    pub nonce: u64,
    pub deadline: u64,
}

/// Exact values an EVM signer needs for EIP-712 Withdraw typed data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawTypedData {
    pub name: &'static str,
    pub version: &'static str,
    pub chain_id: [u8; 32],
    pub salt: [u8; 32],
    pub type_signature: &'static str,
    pub proxy_account: Pubkey,
    pub mint: String,
    pub amount: u64,
    pub decimals: u8,
    pub recipient: String,
    pub generation: u64,
    pub nonce: u64,
    pub deadline: u64,
}

impl WithdrawIntent {
    /// Derive typed data from canonical account keys and immutable Config fields.
    pub fn typed_data(
        &self,
        program_id: &Pubkey,
        store: &Pubkey,
        genesis_hash: &[u8; 32],
    ) -> WithdrawTypedData {
        let proxy_account = crate::pda::find_proxy_address(&self.evm_address, program_id).0;
        WithdrawTypedData {
            name: gmsol_evm_proxy_intent::DOMAIN_NAME,
            version: gmsol_evm_proxy_intent::DOMAIN_VERSION,
            chain_id: self.chain_id,
            salt: gmsol_evm_proxy_intent::domain_salt(program_id, store, genesis_hash),
            type_signature: gmsol_evm_proxy_intent::WITHDRAW_TYPE,
            proxy_account,
            mint: self.mint.to_string(),
            amount: self.amount,
            decimals: self.decimals,
            recipient: self.recipient.to_string(),
            generation: self.generation,
            nonce: self.nonce,
            deadline: self.deadline,
        }
    }

    /// Return the exact digest verified on chain for this Config and deployment.
    pub fn digest(&self, program_id: &Pubkey, store: &Pubkey, genesis_hash: &[u8; 32]) -> [u8; 32] {
        let proxy = crate::pda::find_proxy_address(&self.evm_address, program_id).0;
        let domain = gmsol_evm_proxy_intent::domain_separator(
            program_id,
            store,
            genesis_hash,
            &self.chain_id,
        );
        let struct_hash =
            gmsol_evm_proxy_intent::withdraw_struct_hash(&gmsol_evm_proxy_intent::WithdrawFields {
                proxy,
                mint: self.mint,
                amount: self.amount,
                decimals: self.decimals,
                recipient: self.recipient,
                generation: self.generation,
                nonce: self.nonce,
                deadline: self.deadline,
            });
        gmsol_evm_proxy_intent::intent_digest(&domain, &struct_hash)
    }
}

/// Build a signed Withdraw against the canonical proxy and legacy SPL token ATAs.
pub fn withdraw_instruction(
    program_id: Pubkey,
    submitter: Pubkey,
    intent: &WithdrawIntent,
    signature: [u8; 64],
    recovery_id: u8,
) -> crate::Result<Instruction> {
    let proxy = crate::pda::find_proxy_address(&intent.evm_address, &program_id).0;
    if intent.amount == 0 {
        return Err(crate::Error::custom("Withdraw amount must be positive"));
    }
    if intent.recipient == proxy {
        return Err(crate::Error::custom("proxy cannot withdraw to itself"));
    }
    if recovery_id > 1 {
        return Err(crate::Error::custom("invalid EVM recovery ID"));
    }
    let token_program = anchor_spl::token::ID;
    Ok(Instruction {
        program_id,
        accounts: accounts::Withdraw {
            submitter,
            config: crate::pda::find_proxy_config_address(&program_id).0,
            proxy,
            mint: intent.mint,
            recipient: intent.recipient,
            source: get_associated_token_address_with_program_id(
                &proxy,
                &intent.mint,
                &token_program,
            ),
            destination: get_associated_token_address_with_program_id(
                &intent.recipient,
                &intent.mint,
                &token_program,
            ),
            token_program,
        }
        .to_account_metas(None),
        data: args::Withdraw {
            evm_address: intent.evm_address,
            chain_id: intent.chain_id,
            amount: intent.amount,
            decimals: intent.decimals,
            generation: intent.generation,
            nonce: intent.nonce,
            deadline: intent.deadline,
            signature,
            recovery_id,
        }
        .data(),
    })
}

/// One decoded Config change event.
#[derive(Debug)]
pub enum ConfigEvent {
    /// Config creation.
    Created(events::ConfigCreated),
    /// Allowed-chain mutation.
    ChainAllowedChanged(events::ConfigChainAllowedChanged),
    /// Pause mutation.
    PauseChanged(events::ConfigPauseChanged),
    /// Pending-admin proposal or replacement.
    AdminProposed(events::ConfigAdminProposed),
    /// Pending-admin acceptance.
    AdminAccepted(events::ConfigAdminAccepted),
}

/// Decode a `Program data:` log produced by the EVM proxy program.
///
/// The caller must associate the log with this program's invocation and count it
/// only when the containing transaction succeeded. Other log lines return `None`.
pub fn decode_config_event_log(line: &str) -> crate::Result<Option<ConfigEvent>> {
    let Some(encoded) = line.strip_prefix("Program data: ") else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    macro_rules! decode {
        ($event:ty, $variant:ident) => {
            if data.starts_with(<$event>::DISCRIMINATOR) {
                return crate::utils::events::decode_anchor_event_with_options::<$event>(
                    &data, false,
                )
                .map(ConfigEvent::$variant)
                .map(Some);
            }
        };
    }
    decode!(events::ConfigCreated, Created);
    decode!(events::ConfigChainAllowedChanged, ChainAllowedChanged);
    decode!(events::ConfigPauseChanged, PauseChanged);
    decode!(events::ConfigAdminProposed, AdminProposed);
    decode!(events::ConfigAdminAccepted, AdminAccepted);
    Ok(None)
}

/// Decode a consumed intent from a successful EVM proxy transaction's log.
pub fn decode_intent_event_log(line: &str) -> crate::Result<Option<events::IntentConsumed>> {
    let Some(encoded) = line.strip_prefix("Program data: ") else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    if !data.starts_with(events::IntentConsumed::DISCRIMINATOR) {
        return Ok(None);
    }
    crate::utils::events::decode_anchor_event_with_options::<events::IntentConsumed>(&data, false)
        .map(Some)
}

/// Decode one successful Withdraw event after transaction success is confirmed.
pub fn decode_withdraw_event_log(line: &str) -> crate::Result<Option<events::Withdrawn>> {
    let Some(encoded) = line.strip_prefix("Program data: ") else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    if !data.starts_with(events::Withdrawn::DISCRIMINATOR) {
        return Ok(None);
    }
    crate::utils::events::decode_anchor_event_with_options::<events::Withdrawn>(&data, false)
        .map(Some)
}

#[cfg(test)]
mod tests {
    use anchor_lang::{AnchorDeserialize, Event};
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn decodes_config_event_and_ignores_other_logs() {
        let config = Pubkey::new_unique();
        let event = events::ConfigPauseChanged {
            config,
            previous_paused: true,
            paused: false,
        };
        let log = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(event.data())
        );
        let Some(ConfigEvent::PauseChanged(decoded)) = decode_config_event_log(&log).unwrap()
        else {
            panic!("expected pause change event");
        };
        assert_eq!(decoded.config, config);
        assert!(decoded.previous_paused);
        assert!(!decoded.paused);
        assert!(decode_config_event_log("Program log: other")
            .unwrap()
            .is_none());
    }

    #[test]
    fn decodes_consumed_pair_event() {
        let event = events::IntentConsumed {
            proxy: Pubkey::new_unique(),
            generation: 8,
            nonce: 0,
            kind: 1,
        };
        let log = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(event.data())
        );
        let decoded = decode_intent_event_log(&log).unwrap().unwrap();
        assert_eq!(decoded.proxy, event.proxy);
        assert_eq!(decoded.generation, event.generation);
        assert_eq!(decoded.nonce, event.nonce);
        assert_eq!(decoded.kind, event.kind);
        assert!(decode_config_event_log(&log).unwrap().is_none());
    }

    #[test]
    fn withdraw_builder_binds_canonical_accounts_and_signed_generation() {
        let program = gmsol_programs::gmsol_evm_proxy::ID;
        let submitter = Pubkey::new_unique();
        let intent = WithdrawIntent {
            evm_address: [7; 20],
            chain_id: [0; 32],
            mint: Pubkey::new_unique(),
            amount: 42,
            decimals: 6,
            recipient: Pubkey::new_unique(),
            generation: 8,
            nonce: 0,
            deadline: 500,
        };
        let ix = withdraw_instruction(program, submitter, &intent, [3; 64], 1).unwrap();
        let proxy = crate::pda::find_proxy_address(&intent.evm_address, &program).0;
        let token_program = anchor_spl::token::ID;
        assert_eq!(ix.program_id, program);
        assert_eq!(ix.accounts[0].pubkey, submitter);
        assert_eq!(ix.accounts[2].pubkey, proxy);
        assert_eq!(
            ix.accounts[5].pubkey,
            get_associated_token_address_with_program_id(&proxy, &intent.mint, &token_program)
        );
        assert_eq!(
            ix.accounts[6].pubkey,
            get_associated_token_address_with_program_id(
                &intent.recipient,
                &intent.mint,
                &token_program
            )
        );
        assert_eq!(ix.accounts[7].pubkey, token_program);
        assert_eq!(&ix.data[..8], args::Withdraw::DISCRIMINATOR);
        let data = args::Withdraw::deserialize(&mut &ix.data[8..]).unwrap();
        assert_eq!((data.generation, data.nonce, data.amount), (8, 0, 42));
        let store = Pubkey::new_unique();
        let typed = intent.typed_data(&program, &store, &[9; 32]);
        assert_eq!(typed.proxy_account, proxy);
        assert_eq!(typed.type_signature, gmsol_evm_proxy_intent::WITHDRAW_TYPE);
        assert_eq!(typed.mint, intent.mint.to_string());
        assert_eq!(typed.recipient, intent.recipient.to_string());
        assert_eq!(typed.generation, 8);
        assert_ne!(
            intent.digest(&program, &store, &[9; 32]),
            WithdrawIntent {
                generation: 9,
                ..intent
            }
            .digest(&program, &store, &[9; 32])
        );
    }

    #[test]
    fn decodes_withdraw_event() {
        let event = events::Withdrawn {
            proxy: Pubkey::new_unique(),
            generation: 3,
            nonce: 5,
            mint: Pubkey::new_unique(),
            amount: 42,
            recipient: Pubkey::new_unique(),
        };
        let log = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(event.data())
        );
        let decoded = decode_withdraw_event_log(&log).unwrap().unwrap();
        assert_eq!(
            (
                decoded.proxy,
                decoded.generation,
                decoded.nonce,
                decoded.mint,
                decoded.amount,
                decoded.recipient
            ),
            (event.proxy, 3, 5, event.mint, 42, event.recipient)
        );
        assert!(decode_withdraw_event_log("Program log: other")
            .unwrap()
            .is_none());
    }

    #[test]
    fn rust_typed_data_and_digest_match_independent_vector() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../programs/evm-proxy/tests/fixtures/withdraw-vectors.json"
        ))
        .unwrap();
        let vector = &fixture["vectors"][0];
        let bytes = |name: &str| {
            hex::decode(vector[name].as_str().unwrap().trim_start_matches("0x")).unwrap()
        };
        let array = |name: &str| -> [u8; 32] { bytes(name).try_into().unwrap() };
        let program = Pubkey::new_from_array(array("program"));
        let store = Pubkey::new_from_array(array("store"));
        let intent = WithdrawIntent {
            evm_address: bytes("evmAddress").try_into().unwrap(),
            chain_id: array("chainId"),
            mint: Pubkey::new_from_array(array("mint")),
            amount: vector["amount"].as_str().unwrap().parse().unwrap(),
            decimals: vector["decimals"].as_u64().unwrap() as u8,
            recipient: Pubkey::new_from_array(array("recipient")),
            generation: vector["generation"].as_str().unwrap().parse().unwrap(),
            nonce: vector["nonce"].as_str().unwrap().parse().unwrap(),
            deadline: vector["deadline"].as_str().unwrap().parse().unwrap(),
        };
        let genesis = array("genesis");
        let typed = intent.typed_data(&program, &store, &genesis);
        assert_eq!(typed.proxy_account.to_bytes(), array("proxy"));
        assert_eq!(typed.salt, array("salt"));
        assert_eq!(typed.mint, vector["mintBase58"]);
        assert_eq!(typed.recipient, vector["recipientBase58"]);
        assert_eq!(intent.digest(&program, &store, &genesis), array("digest"));
    }
}
