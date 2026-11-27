//! Signed EVM proxy intent data and instructions.

/// Owner-aware accounts and committed CPI events for the selected Proxy deployment.
#[cfg(feature = "decode")]
pub use gmsol_decode::gmsol::evm_proxy::{
    decode_committed_events, decode_config_account, ProxyCPIEvent,
};

use crate::{builders::evm_proxy::EvmProxyProgram, pda::{EvmAddressBytes, ReferralCodeBytes}};
use base64::Engine;
use gmsol_programs::gmsol_evm_proxy::events;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_evm_proxy::client::{accounts, args};
use gmsol_solana_utils::ProgramExt;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Signed registration fields. Code bytes are normalized before signing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetReferralCodeIntent {
    /// EVM owner used to derive the proxy.
    pub evm_address: EvmAddressBytes,
    /// Signed full-width EVM chain ID.
    pub chain_id: [u8; 32],
    /// Canonical eight-byte referral code.
    pub code: ReferralCodeBytes,
    /// Signed uint64 counter value.
    pub nonce: u64,
    /// Signed execution deadline.
    pub deadline: u64,
}

/// Exact EIP-712 values for referral registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetReferralCodeTypedData {
    /// EIP-712 domain name.
    pub name: &'static str,
    /// EIP-712 domain version.
    pub version: &'static str,
    /// Signed full-width EVM chain ID.
    pub chain_id: [u8; 32],
    /// Deployment, Store and genesis binding.
    pub salt: [u8; 32],
    /// Canonical SetReferralCode type definition.
    pub type_signature: &'static str,
    /// Permanent proxy bound by this signature.
    pub proxy_account: Pubkey,
    /// Canonical referral-code bytes or their signable string representation.
    pub code: String,
    /// Signed uint64 counter value.
    pub nonce: u64,
    /// Signed execution deadline.
    pub deadline: u64,
}

impl SetReferralCodeIntent {
    /// Normalize aliases to the only string accepted by the on-chain verifier.
    pub fn typed_data(
        &self,
        program_id: &Pubkey,
        store: &Pubkey,
        genesis_hash: &[u8; 32],
    ) -> SetReferralCodeTypedData {
        SetReferralCodeTypedData {
            name: gmsol_evm_proxy_intent::DOMAIN_NAME,
            version: gmsol_evm_proxy_intent::DOMAIN_VERSION,
            chain_id: self.chain_id,
            salt: gmsol_evm_proxy_intent::domain_salt(program_id, store, genesis_hash),
            type_signature: gmsol_evm_proxy_intent::SET_REFERRAL_CODE_TYPE,
            proxy_account: crate::pda::find_proxy_address(&self.evm_address, program_id).0,
            code: gmsol_programs::gmsol_store::accounts::ReferralCodeV2::encode(&self.code, true),
            nonce: self.nonce,
            deadline: self.deadline,
        }
    }

    /// Digest over the canonical code, complete nonce and deployment domain.
    pub fn digest(&self, program_id: &Pubkey, store: &Pubkey, genesis_hash: &[u8; 32]) -> [u8; 32] {
        let proxy = crate::pda::find_proxy_address(&self.evm_address, program_id).0;
        let domain = gmsol_evm_proxy_intent::domain_separator(
            program_id,
            store,
            genesis_hash,
            &self.chain_id,
        );
        let structure = gmsol_evm_proxy_intent::set_referral_code_struct_hash(
            &proxy,
            &gmsol_programs::gmsol_store::accounts::ReferralCodeV2::encode(&self.code, true),
            self.nonce,
            self.deadline,
        );
        gmsol_evm_proxy_intent::intent_digest(&domain, &structure)
    }
}

/// Build registration with canonical Store PDAs, including a missing User.
/// The submitter signs and sponsors rent; the transaction fee payer may differ.
pub fn set_referral_code_instruction(
    program_id: Pubkey,
    submitter: Pubkey,
    store: Pubkey,
    intent: &SetReferralCodeIntent,
    signature: [u8; 64],
    recovery_id: u8,
) -> crate::Result<Instruction> {
    if intent.code == [0; 8] {
        return Err(crate::Error::custom("referral code cannot be zero"));
    }
    if recovery_id > 1 {
        return Err(crate::Error::custom("invalid EVM recovery ID"));
    }
    let proxy = crate::pda::find_proxy_address(&intent.evm_address, &program_id).0;
    let store_program = gmsol_programs::gmsol_store::ID;
    Ok(EvmProxyProgram::from(program_id)
        .anchor_instruction(args::SetReferralCode {
            chain_id: intent.chain_id,
            code: intent.code,
            nonce: intent.nonce,
            deadline: intent.deadline,
            signature,
            recovery_id,
        })
        .anchor_accounts(
            accounts::SetReferralCode {
                submitter,
                config: crate::pda::find_proxy_config_address(&program_id).0,
                proxy,
                store,
                user: crate::pda::find_user_address(&store, &proxy, &store_program).0,
                referral_code: crate::pda::find_referral_code_address(
                    &store,
                    intent.code,
                    &store_program,
                )
                .0,
                store_program,
                system_program: solana_sdk::system_program::ID,
            },
            false,
        )
        .build())
}

/// Decode only after confirming that the enclosing transaction succeeded.
pub fn decode_referral_code_event_log(
    line: &str,
) -> crate::Result<Option<events::ReferralCodeSet>> {
    decode_program_data(line)
}

fn decode_program_data<E: anchor_lang::Event>(line: &str) -> crate::Result<Option<E>> {
    let Some(encoded) = line.strip_prefix("Program data: ") else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    if !data.starts_with(E::DISCRIMINATOR) {
        return Ok(None);
    }
    crate::utils::events::decode_anchor_event_with_options::<E>(&data, false).map(Some)
}

/// Signed fields of one Withdraw intent, using the proxy's shared next nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WithdrawIntent {
    /// EVM owner used to derive the proxy.
    pub evm_address: EvmAddressBytes,
    /// Signed full-width EVM chain ID.
    pub chain_id: [u8; 32],
    /// Signed mint, encoded as a base58 string in EIP-712.
    pub mint: Pubkey,
    /// Signed transfer amount in base units.
    pub amount: u64,
    /// Signed mint precision.
    pub decimals: u8,
    /// Signed recipient wallet, encoded as a base58 string in EIP-712.
    pub recipient: Pubkey,
    /// Signed uint64 counter value.
    pub nonce: u64,
    /// Signed execution deadline.
    pub deadline: u64,
}

/// Exact values an EVM signer needs for EIP-712 Withdraw typed data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawTypedData {
    /// EIP-712 domain name.
    pub name: &'static str,
    /// EIP-712 domain version.
    pub version: &'static str,
    /// Signed full-width EVM chain ID.
    pub chain_id: [u8; 32],
    /// Deployment, Store and genesis binding.
    pub salt: [u8; 32],
    /// Canonical Withdraw type definition.
    pub type_signature: &'static str,
    /// Permanent proxy bound by this signature.
    pub proxy_account: Pubkey,
    /// Signed mint, encoded as a base58 string in EIP-712.
    pub mint: String,
    /// Signed transfer amount in base units.
    pub amount: u64,
    /// Signed mint precision.
    pub decimals: u8,
    /// Signed recipient wallet, encoded as a base58 string in EIP-712.
    pub recipient: String,
    /// Signed uint64 counter value.
    pub nonce: u64,
    /// Signed execution deadline.
    pub deadline: u64,
}

impl WithdrawIntent {
    /// Validate executable transfer fields and return the selected proxy address.
    ///
    /// # Errors
    /// Returns an error for a zero amount or a recipient equal to this proxy.
    pub(crate) fn validate(&self, program_id: &Pubkey) -> crate::Result<Pubkey> {
        let proxy = crate::pda::find_proxy_address(&self.evm_address, program_id).0;
        if self.amount == 0 {
            return Err(crate::Error::custom("Withdraw amount must be positive"));
        }
        if self.recipient == proxy {
            return Err(crate::Error::custom("proxy cannot withdraw to itself"));
        }
        Ok(proxy)
    }

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
    let proxy = intent.validate(&program_id)?;
    if recovery_id > 1 {
        return Err(crate::Error::custom("invalid EVM recovery ID"));
    }
    let token_program = anchor_spl::token::ID;
    Ok(EvmProxyProgram::from(program_id)
        .anchor_instruction(args::Withdraw {
            chain_id: intent.chain_id,
            amount: intent.amount,
            decimals: intent.decimals,
            nonce: intent.nonce,
            deadline: intent.deadline,
            signature,
            recovery_id,
        })
        .anchor_accounts(
            accounts::Withdraw {
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
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build())
}

/// Signed fields for one Cancel operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelIntent {
    /// EVM owner used to derive the permanent proxy account.
    pub evm_address: EvmAddressBytes,
    /// Full-width EVM chain ID.
    pub chain_id: [u8; 32],
    /// Signed next expected nonce, initially zero, encoded as uint64.
    pub nonce: u64,
    /// Signed execution deadline.
    pub deadline: u64,
}

/// Exact EIP-712 fields presented to the EVM signer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelTypedData {
    /// EIP-712 domain name.
    pub name: &'static str,
    /// EIP-712 domain version.
    pub version: &'static str,
    /// Full-width EVM chain ID.
    pub chain_id: [u8; 32],
    /// Deployment, Store and genesis binding.
    pub salt: [u8; 32],
    /// Exact EIP-712 primary-type definition.
    pub type_signature: &'static str,
    /// Permanent proxy account bound by this message.
    pub proxy_account: Pubkey,
    /// Signed next expected nonce, initially zero, encoded as uint64.
    pub nonce: u64,
    /// Signed execution deadline.
    pub deadline: u64,
}

impl CancelIntent {
    /// Build typed fields from the same canonical encoding used by the program.
    pub fn typed_data(
        &self,
        program_id: &Pubkey,
        store: &Pubkey,
        genesis_hash: &[u8; 32],
    ) -> CancelTypedData {
        CancelTypedData {
            name: gmsol_evm_proxy_intent::DOMAIN_NAME,
            version: gmsol_evm_proxy_intent::DOMAIN_VERSION,
            chain_id: self.chain_id,
            salt: gmsol_evm_proxy_intent::domain_salt(program_id, store, genesis_hash),
            type_signature: gmsol_evm_proxy_intent::CANCEL_TYPE,
            proxy_account: crate::pda::find_proxy_address(&self.evm_address, program_id).0,
            nonce: self.nonce,
            deadline: self.deadline,
        }
    }

    /// Digest that the on-chain verifier recovers against the EVM address.
    pub fn digest(&self, program_id: &Pubkey, store: &Pubkey, genesis_hash: &[u8; 32]) -> [u8; 32] {
        let proxy = crate::pda::find_proxy_address(&self.evm_address, program_id).0;
        let domain = gmsol_evm_proxy_intent::domain_separator(
            program_id,
            store,
            genesis_hash,
            &self.chain_id,
        );
        let fields = gmsol_evm_proxy_intent::cancel_struct_hash(&proxy, self.nonce, self.deadline);
        gmsol_evm_proxy_intent::intent_digest(&domain, &fields)
    }
}

/// Build a Cancel instruction against the canonical Config and proxy PDAs.
pub fn cancel_instruction(
    program_id: Pubkey,
    submitter: Pubkey,
    intent: &CancelIntent,
    signature: [u8; 64],
    recovery_id: u8,
) -> crate::Result<Instruction> {
    if recovery_id > 1 {
        return Err(crate::Error::custom("invalid EVM recovery ID"));
    }
    Ok(EvmProxyProgram::from(program_id)
        .anchor_instruction(args::Cancel {
            chain_id: intent.chain_id,
            nonce: intent.nonce,
            deadline: intent.deadline,
            signature,
            recovery_id,
        })
        .anchor_accounts(
            accounts::Cancel {
                submitter,
                config: crate::pda::find_proxy_config_address(&program_id).0,
                proxy: crate::pda::find_proxy_address(&intent.evm_address, &program_id).0,
                event_authority: EvmProxyProgram::from(program_id).find_event_authority_address(),
                program: program_id,
            },
            false,
        )
        .build())
}

#[cfg(test)]
mod tests {
    mod referral_tests {
        use super::*;
        use anchor_lang::{AnchorDeserialize, Discriminator};
        #[test]
        fn referral_builder_and_digest_match_independent_vectors() {
            const SUBMITTER_ACCOUNT_INDEX: usize = 0;
            const USER_ACCOUNT_INDEX: usize = 4;
            const REFERRAL_ACCOUNT_INDEX: usize = 5;
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../evm-proxy-intent/fixtures/referral-vectors.json"
            ))
            .unwrap();
            let bytes = |key: &str| {
                hex::decode(fixture[key].as_str().unwrap().trim_start_matches("0x")).unwrap()
            };
            let program = Pubkey::new_from_array(bytes("program").try_into().unwrap());
            let store = Pubkey::new_from_array(bytes("store").try_into().unwrap());
            let genesis = bytes("genesis").try_into().unwrap();
            let submitter = Pubkey::new_unique();
            for vector in fixture["vectors"].as_array().unwrap() {
                let intent = SetReferralCodeIntent {
                    evm_address: bytes("evmAddress").try_into().unwrap(),
                    chain_id: hex::decode(
                        vector["chainId"].as_str().unwrap().trim_start_matches("0x"),
                    )
                    .unwrap()
                    .try_into()
                    .unwrap(),
                    code: hex::decode(vector["bytesHex"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                    nonce: vector["nonce"].as_str().unwrap().parse().unwrap(),
                    deadline: vector["deadline"].as_str().unwrap().parse().unwrap(),
                };
                assert_eq!(
                    hex::encode(intent.digest(&program, &store, &genesis)),
                    vector["digest"].as_str().unwrap().trim_start_matches("0x")
                );
                assert_eq!(
                    intent.typed_data(&program, &store, &genesis).code,
                    vector["canonical"]
                );
                if intent.code == [0; 8] {
                    assert!(set_referral_code_instruction(
                        program, submitter, store, &intent, [1; 64], 0
                    )
                    .is_err());
                    continue;
                }
                let ix =
                    set_referral_code_instruction(program, submitter, store, &intent, [1; 64], 0)
                        .unwrap();
                let data = args::SetReferralCode::deserialize(
                    &mut &ix.data[args::SetReferralCode::DISCRIMINATOR.len()..],
                )
                .unwrap();
                assert_eq!(
                    (data.code, data.nonce, data.deadline),
                    (intent.code, intent.nonce, intent.deadline)
                );
                assert_eq!(ix.accounts[SUBMITTER_ACCOUNT_INDEX].pubkey, submitter);
                let proxy = crate::pda::find_proxy_address(&intent.evm_address, &program).0;
                assert_eq!(
                    ix.accounts[USER_ACCOUNT_INDEX].pubkey,
                    crate::pda::find_user_address(&store, &proxy, &gmsol_programs::gmsol_store::ID)
                        .0
                );
                assert_eq!(
                    ix.accounts[REFERRAL_ACCOUNT_INDEX].pubkey,
                    crate::pda::find_referral_code_address(
                        &store,
                        intent.code,
                        &gmsol_programs::gmsol_store::ID
                    )
                    .0
                );
            }
        }
    }
    use super::*;
    use anchor_lang::{AnchorDeserialize, Discriminator};
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn withdraw_builder_binds_canonical_accounts_and_signed_nonce() {
        let program = Pubkey::new_unique();
        let submitter = Pubkey::new_unique();
        let intent = WithdrawIntent {
            evm_address: [7; 20],
            chain_id: [0; 32],
            mint: Pubkey::new_unique(),
            amount: 42,
            decimals: 6,
            recipient: Pubkey::new_unique(),
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
        assert_eq!(ix.accounts.len(), 10);
        let authority = Pubkey::find_program_address(&[b"__event_authority"], &program).0;
        assert_eq!(
            ix.accounts[8],
            solana_sdk::instruction::AccountMeta::new_readonly(authority, false)
        );
        assert_eq!(
            ix.accounts[9],
            solana_sdk::instruction::AccountMeta::new_readonly(program, false)
        );
        let data =
            args::Withdraw::deserialize(&mut &ix.data[args::Withdraw::DISCRIMINATOR.len()..])
                .unwrap();
        assert_eq!((data.nonce, data.amount), (0, 42));
        let store = Pubkey::new_unique();
        let typed = intent.typed_data(&program, &store, &[9; 32]);
        assert_eq!(typed.proxy_account, proxy);
        assert_eq!(typed.type_signature, gmsol_evm_proxy_intent::WITHDRAW_TYPE);
        assert_eq!(typed.mint, intent.mint.to_string());
        assert_eq!(typed.recipient, intent.recipient.to_string());
        assert_eq!(typed.nonce, 0);
        assert_ne!(
            intent.digest(&program, &store, &[9; 32]),
            WithdrawIntent { nonce: 1, ..intent }.digest(&program, &store, &[9; 32])
        );
    }

    #[test]
    fn rust_typed_data_and_digest_match_independent_vector() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../evm-proxy-intent/fixtures/withdraw-vectors.json"
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

    use serde_json::Value;

    fn bytes<const N: usize>(value: &str) -> [u8; N] {
        hex::decode(value.strip_prefix("0x").unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }

    #[test]
    fn cancel_builder_derives_cpi_accounts_for_selected_program() {
        let program = Pubkey::new_unique();
        let submitter = Pubkey::new_unique();
        let intent = CancelIntent {
            evm_address: [0x42; 20],
            chain_id: [1; 32],
            nonce: 0,
            deadline: 7200,
        };
        let instruction = cancel_instruction(program, submitter, &intent, [1; 64], 0).unwrap();
        assert_eq!(instruction.program_id, program);
        assert_eq!(instruction.accounts.len(), 5);
        let expected_authority = Pubkey::find_program_address(&[b"__event_authority"], &program).0;
        assert_eq!(
            instruction.accounts[3],
            solana_sdk::instruction::AccountMeta::new_readonly(expected_authority, false)
        );
        assert_eq!(
            instruction.accounts[4],
            solana_sdk::instruction::AccountMeta::new_readonly(program, false)
        );
        let decoded =
            args::Cancel::deserialize(&mut &instruction.data[args::Cancel::DISCRIMINATOR.len()..])
                .unwrap();
        assert_eq!(decoded.nonce, 0);
    }

    #[test]
    fn cancel_vectors_bind_nonce_typed_data_and_instruction() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../evm-proxy-intent/fixtures/cancel-vectors.json"
        ))
        .unwrap();
        for name in ["first", "max", "one", "large-nonce"] {
            let vector = fixture["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .unwrap();
            let program = Pubkey::new_from_array(bytes(vector["program"].as_str().unwrap()));
            let store = Pubkey::new_from_array(bytes(vector["store"].as_str().unwrap()));
            let genesis = bytes(vector["genesis"].as_str().unwrap());
            let intent = CancelIntent {
                evm_address: bytes(fixture["evmAddress"].as_str().unwrap()),
                chain_id: bytes(vector["chainId"].as_str().unwrap()),
                nonce: vector["nonce"].as_str().unwrap().parse().unwrap(),
                deadline: vector["deadline"].as_str().unwrap().parse().unwrap(),
            };
            let typed = intent.typed_data(&program, &store, &genesis);
            assert_eq!(
                typed.type_signature,
                "Cancel(bytes32 proxyAccount,uint64 nonce,uint64 deadline)"
            );
            assert_eq!(typed.nonce, intent.nonce);
            assert_eq!(
                typed.proxy_account.to_bytes(),
                bytes::<32>(vector["proxyAccount"].as_str().unwrap())
            );
            assert_eq!(
                intent.digest(&program, &store, &genesis),
                bytes::<32>(vector["digest"].as_str().unwrap())
            );
            let submitter = Pubkey::new_unique();
            let ix = cancel_instruction(program, submitter, &intent, [1; 64], 0).unwrap();
            let decoded =
                args::Cancel::deserialize(&mut &ix.data[args::Cancel::DISCRIMINATOR.len()..])
                    .unwrap();
            assert_eq!(
                (decoded.nonce, decoded.deadline),
                (intent.nonce, intent.deadline)
            );
            assert_eq!(ix.accounts[0].pubkey, submitter);
            assert_eq!(
                ix.accounts[1].pubkey,
                crate::pda::find_proxy_config_address(&program).0
            );
            assert_eq!(ix.accounts[2].pubkey, typed.proxy_account);
        }
    }
}
