use std::collections::{HashMap, HashSet};

use anchor_lang::prelude::{event::EVENT_IX_TAG_LE, AccountMeta};
use base64::{engine::general_purpose::STANDARD, Engine};
use solana_sdk::{
    instruction::CompiledInstruction, message::v0::MessageAddressTableLookup, pubkey::Pubkey,
    signature::Signature, transaction::VersionedTransaction,
};
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, EncodedTransaction, EncodedTransactionWithStatusMeta,
    TransactionBinaryEncoding, UiInstruction, UiTransactionStatusMeta,
};

use crate::{Decode, DecodeError, Decoder, Visitor};

pub use solana_transaction_status_client_types as solana_transaction_status;

pub use super::v1_codec::{V1Message, V1Transaction, V1TransactionConfig};

#[cfg(test)]
#[path = "solana_decoder_tests.rs"]
mod tests;

/// Transaction Decoder.
pub struct TransactionDecoder<'a> {
    slot: u64,
    signature: Signature,
    transaction: &'a EncodedTransactionWithStatusMeta,
    cpi_event_filter: CPIEventFilter,
}

impl<'a> TransactionDecoder<'a> {
    /// Create a new transaction decoder.
    pub fn new(
        slot: u64,
        signature: Signature,
        transaction: &'a EncodedTransactionWithStatusMeta,
    ) -> Self {
        Self {
            slot,
            signature,
            transaction,
            cpi_event_filter: CPIEventFilter {
                map: Default::default(),
            },
        }
    }

    /// Add a Program ID to the CPI Event filter.
    pub fn add_cpi_event_program_id(
        &mut self,
        program_id: &Pubkey,
    ) -> Result<&mut Self, DecodeError> {
        self.cpi_event_filter.add(program_id)?;
        Ok(self)
    }

    /// Add a Event authority and its Program ID to the CPI Event filter.
    pub fn add_cpi_event_authority_and_program_id(
        &mut self,
        event_authority: Pubkey,
        program_id: Pubkey,
    ) -> Result<&mut Self, DecodeError> {
        self.cpi_event_filter
            .add_event_authority_and_program_id(event_authority, program_id)?;
        Ok(self)
    }

    /// Set CPI events filter.
    pub fn set_cpi_event_filter(&mut self, filter: CPIEventFilter) -> &mut Self {
        self.cpi_event_filter = filter;
        self
    }

    /// Get signature.
    pub fn signature(&self) -> Signature {
        self.signature
    }

    /// Get slot.
    pub fn slot(&self) -> u64 {
        self.slot
    }

    /// Get transaction.
    pub fn transaction(&self) -> &EncodedTransactionWithStatusMeta {
        self.transaction
    }

    /// Decode a legacy or v0 transaction using the original Solana 2.1 type.
    /// Use [`Self::decoded_transaction_any_version`] to also accept v1.
    pub fn decoded_transaction(&self) -> Result<DecodedTransaction, DecodeError> {
        let tx = self.transaction;
        let slot_index = (self.slot, None);
        let Some(decoded) = tx.transaction.decode() else {
            return Err(DecodeError::custom("failed to decode transaction"));
        };
        let Some(meta) = &tx.meta else {
            return Err(DecodeError::custom("missing meta"));
        };

        let (dynamic_writable_accounts, dynamic_readonly_accounts) = match &meta.loaded_addresses {
            OptionSerializer::Some(loaded) => {
                let dynamic_writable_accounts = loaded
                    .writable
                    .iter()
                    .map(|address| address.parse().map_err(DecodeError::custom))
                    .collect::<Result<Vec<_>, _>>()?;
                let dynamic_readonly_accounts = loaded
                    .readonly
                    .iter()
                    .map(|address| address.parse().map_err(DecodeError::custom))
                    .collect::<Result<Vec<_>, _>>()?;
                (dynamic_writable_accounts, dynamic_readonly_accounts)
            }
            OptionSerializer::None | OptionSerializer::Skip => Default::default(),
        };

        Ok(DecodedTransaction {
            signature: self.signature,
            slot_index,
            transaction: decoded,
            dynamic_writable_accounts,
            dynamic_readonly_accounts,
            transaction_status_meta: meta,
        })
    }

    /// Decode legacy, v0, or v1 without changing the original decoded type.
    ///
    /// Like [`Self::decoded_transaction`], this requires RPC execution meta.
    /// Decoding and structural checks do not verify signatures or authorize execution.
    pub fn decoded_transaction_any_version(
        &self,
    ) -> Result<SupportedDecodedTransaction<'_>, DecodeError> {
        let bytes = match &self.transaction.transaction {
            EncodedTransaction::LegacyBinary(blob)
            | EncodedTransaction::Binary(blob, TransactionBinaryEncoding::Base58) => {
                bs58::decode(blob).into_vec().map_err(DecodeError::custom)?
            }
            EncodedTransaction::Binary(blob, TransactionBinaryEncoding::Base64) => {
                STANDARD.decode(blob).map_err(DecodeError::custom)?
            }
            _ => return Err(DecodeError::custom("expected a binary encoded transaction")),
        };
        if bytes.first() != Some(&super::v1_codec::PREFIX) {
            return self
                .decoded_transaction()
                .map(SupportedDecodedTransaction::LegacyOrV0);
        }
        let transaction = super::v1_codec::decode(&bytes)?;
        let meta = self
            .transaction
            .meta
            .as_ref()
            .ok_or_else(|| DecodeError::custom("missing meta"))?;
        if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
            if !loaded.writable.is_empty() || !loaded.readonly.is_empty() {
                return Err(DecodeError::custom("loaded addresses on a v1 transaction"));
            }
        }
        Ok(SupportedDecodedTransaction::V1(DecodedV1Transaction {
            signature: self.signature,
            slot_index: (self.slot, None),
            transaction,
            transaction_status_meta: meta,
        }))
    }

    /// Extract CPI events from legacy, v0, or v1.
    pub fn extract_cpi_events(&self) -> Result<CPIEvents, DecodeError> {
        self.decoded_transaction_any_version()?
            .extract_cpi_events(&self.cpi_event_filter)
    }
}

impl Decoder for TransactionDecoder<'_> {
    fn decode_account<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::custom(
            "Expecting `Account` but found `Transaction`",
        ))
    }

    fn decode_transaction<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_transaction(self.decoded_transaction_any_version()?)
    }

    fn decode_anchor_cpi_events<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_anchor_cpi_events(self.extract_cpi_events()?.access())
    }

    fn decode_owned_data<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::custom(
            "cannot access ownedd data directly of a transaction",
        ))
    }

    fn decode_bytes<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::custom(
            "cannot access bytes directly of a transaction",
        ))
    }
}

/// CPI Event filter.
#[derive(Debug, Clone, Default)]
pub struct CPIEventFilter {
    /// A mapping from event authority to its program id.
    map: HashMap<Pubkey, Pubkey>,
}

impl CPIEventFilter {
    /// Subscribe to CPI Event from the given program.
    pub fn add(&mut self, program_id: &Pubkey) -> Result<&mut Self, DecodeError> {
        let event_authority = find_event_authority_address(program_id);
        self.add_event_authority_and_program_id(event_authority, *program_id)
    }

    /// Add event authority and its program id directly.
    pub fn add_event_authority_and_program_id(
        &mut self,
        event_authority: Pubkey,
        program_id: Pubkey,
    ) -> Result<&mut Self, DecodeError> {
        if let Some(previous) = self.map.insert(event_authority, program_id) {
            // This should be rare, but if a collision does occur, an error will be thrown.
            if previous != program_id {
                return Err(DecodeError::custom(format!(
                    "event authority collision, previous={previous}, current={program_id}"
                )));
            }
        }
        Ok(self)
    }

    /// Get event authorities.
    pub fn event_authorities(&self) -> impl Iterator<Item = &Pubkey> {
        self.map.keys()
    }

    /// Get programs.
    pub fn programs(&self) -> impl Iterator<Item = &Pubkey> {
        self.map.values()
    }
}

/// CPI Event decoder.
pub struct CPIEvent {
    program_id: Pubkey,
    data: Vec<u8>,
}

impl CPIEvent {
    /// Create a new [`CPIEvent`] decoder.
    pub fn new(program_id: Pubkey, data: Vec<u8>) -> Self {
        Self { program_id, data }
    }
}

impl Decoder for &CPIEvent {
    fn decode_account<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::InvalidType(
            "Expecting `Account` but found `CPIEvent`".to_string(),
        ))
    }

    fn decode_transaction<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::InvalidType(
            "Expecting `Transaction` but found `CPIEvent`".to_string(),
        ))
    }

    fn decode_anchor_cpi_events<V>(&self, _visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        Err(DecodeError::InvalidType(
            "Expecting `AnchorCPIEvents` but found `CPIEvent`".to_string(),
        ))
    }

    fn decode_owned_data<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_owned_data(&self.program_id, &self.data)
    }

    fn decode_bytes<V>(&self, visitor: V) -> Result<V::Value, DecodeError>
    where
        V: Visitor,
    {
        visitor.visit_bytes(&self.data)
    }
}

/// Slot and index.
pub type SlotAndIndex = (u64, Option<usize>);

/// CPI Events.
pub struct CPIEvents {
    /// Signature.
    pub signature: Signature,
    /// Slot and index.
    pub slot_index: SlotAndIndex,
    /// CPI Events.
    pub events: Vec<CPIEvent>,
}

impl CPIEvents {
    /// Access CPI Events.
    pub fn access(&self) -> AccessCPIEvents {
        AccessCPIEvents {
            signature: &self.signature,
            slot_index: &self.slot_index,
            events: self.events.iter(),
        }
    }
}

/// Access CPI Events.
pub struct AccessCPIEvents<'a> {
    signature: &'a Signature,
    slot_index: &'a SlotAndIndex,
    events: std::slice::Iter<'a, CPIEvent>,
}

impl<'a> AccessCPIEvents<'a> {
    /// Create a new access for CPI Events.
    pub fn new(
        signature: &'a Signature,
        slot_index: &'a SlotAndIndex,
        events: &'a [CPIEvent],
    ) -> Self {
        Self {
            signature,
            slot_index,
            events: events.iter(),
        }
    }
}

impl<'a> crate::AnchorCPIEventsAccess<'a> for AccessCPIEvents<'a> {
    fn slot(&self) -> Result<u64, DecodeError> {
        Ok(self.slot_index.0)
    }

    fn index(&self) -> Result<Option<usize>, DecodeError> {
        Ok(self.slot_index.1)
    }

    fn signature(&self) -> Result<&Signature, DecodeError> {
        Ok(self.signature)
    }

    fn next_event<T>(&mut self) -> Result<Option<T>, DecodeError>
    where
        T: Decode,
    {
        let Some(decoder) = self.events.next() else {
            return Ok(None);
        };
        T::decode(decoder).map(Some)
    }
}

const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";

fn find_event_authority_address(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], program_id).0
}

/// Decoded Transaction.
pub struct DecodedTransaction<'a> {
    /// Signature.
    pub signature: Signature,
    /// Slot and index.
    pub slot_index: SlotAndIndex,
    /// Transaction.
    pub transaction: VersionedTransaction,
    /// Dynamic writable accounts.
    pub dynamic_writable_accounts: Vec<Pubkey>,
    /// Dynamic read-only accounts.
    pub dynamic_readonly_accounts: Vec<Pubkey>,
    /// Transaction status meta.
    pub transaction_status_meta: &'a UiTransactionStatusMeta,
}

impl DecodedTransaction<'_> {
    /// Extract Anchor CPI events.
    pub fn extract_cpi_events(
        &self,
        cpi_event_filter: &CPIEventFilter,
    ) -> Result<CPIEvents, DecodeError> {
        let mut accounts = self.transaction.message.static_account_keys().to_vec();
        accounts.extend_from_slice(&self.dynamic_writable_accounts);
        accounts.extend_from_slice(&self.dynamic_readonly_accounts);
        extract_cpi_events(
            self.signature,
            self.slot_index,
            &accounts,
            self.transaction_status_meta,
            cpi_event_filter,
        )
    }
}

fn extract_cpi_events(
    signature: Signature,
    slot_index: SlotAndIndex,
    accounts: &[Pubkey],
    meta: &UiTransactionStatusMeta,
    cpi_event_filter: &CPIEventFilter,
) -> Result<CPIEvents, DecodeError> {
    let mut event_authority_indices = HashMap::<_, HashSet<u8>>::default();
    tracing::debug!("accounts: {accounts:#?}");
    let map = &cpi_event_filter.map;
    for res in accounts
        .iter()
        .enumerate()
        .filter(|(_, key)| map.contains_key(key))
        .map(|(idx, key)| u8::try_from(idx).map(|idx| (map.get(key).unwrap(), idx)))
    {
        let (pubkey, idx) = res.map_err(|_| DecodeError::custom("invalid account keys"))?;
        event_authority_indices
            .entry(pubkey)
            .or_default()
            .insert(idx);
    }
    tracing::debug!("event_authorities: {event_authority_indices:#?}");
    let Some(ixs) = Option::<&Vec<_>>::from(meta.inner_instructions.as_ref()) else {
        return Err(DecodeError::custom("missing inner instructions"));
    };
    let mut events = Vec::default();
    for ix in ixs.iter().flat_map(|ixs| &ixs.instructions) {
        let UiInstruction::Compiled(ix) = ix else {
            tracing::warn!("only compiled instruction is currently supported");
            continue;
        };
        // NOTE: we are currently assuming that the Event CPI has only the event authority in the account list.
        if ix.accounts.len() != 1 {
            continue;
        }
        if let Some(program_id) = accounts.get(ix.program_id_index as usize) {
            let Some(indexes) = event_authority_indices.get(program_id) else {
                continue;
            };
            let data = bs58::decode(&ix.data).into_vec().map_err(|err| {
                DecodeError::custom(format!(
                    "decode ix data error, err={err}. Note that currently only Base58 is supported"
                ))
            })?;
            if indexes.contains(&ix.accounts[0]) && data.starts_with(EVENT_IX_TAG_LE) {
                events.push(CPIEvent::new(*program_id, data));
            }
        }
    }
    Ok(CPIEvents {
        signature,
        slot_index,
        events,
    })
}

/// A complete transaction plus execution context, preserving its actual version.
pub enum SupportedDecodedTransaction<'a> {
    /// The unchanged Solana 2.1 representation for legacy and v0.
    LegacyOrV0(DecodedTransaction<'a>),
    /// A v1 transaction using Solana 2.1 primitive types.
    V1(DecodedV1Transaction<'a>),
}

impl SupportedDecodedTransaction<'_> {
    fn access(&self) -> &dyn crate::TransactionAccess {
        match self {
            Self::LegacyOrV0(tx) => tx,
            Self::V1(tx) => tx,
        }
    }

    /// Extract Anchor CPI events using the same filter for every version.
    pub fn extract_cpi_events(&self, filter: &CPIEventFilter) -> Result<CPIEvents, DecodeError> {
        match self {
            Self::LegacyOrV0(tx) => tx.extract_cpi_events(filter),
            Self::V1(tx) => tx.extract_cpi_events(filter),
        }
    }
}

impl crate::TransactionAccess for SupportedDecodedTransaction<'_> {
    fn slot(&self) -> Result<u64, DecodeError> {
        self.access().slot()
    }
    fn index(&self) -> Result<Option<usize>, DecodeError> {
        self.access().index()
    }
    fn signature(&self) -> Result<&Signature, DecodeError> {
        self.access().signature()
    }
    fn num_signers(&self, is_writable: bool) -> Result<usize, DecodeError> {
        self.access().num_signers(is_writable)
    }
    fn num_accounts(&self) -> usize {
        self.access().num_accounts()
    }
    fn message_signature(&self, idx: usize) -> Option<&Signature> {
        self.access().message_signature(idx)
    }
    fn account_meta(&self, idx: usize) -> Result<Option<AccountMeta>, DecodeError> {
        self.access().account_meta(idx)
    }
    fn num_address_table_lookups(&self) -> usize {
        self.access().num_address_table_lookups()
    }
    fn address_table_lookup(&self, idx: usize) -> Option<&MessageAddressTableLookup> {
        self.access().address_table_lookup(idx)
    }
    fn num_instructions(&self) -> usize {
        self.access().num_instructions()
    }
    fn instruction(&self, idx: usize) -> Option<&CompiledInstruction> {
        self.access().instruction(idx)
    }
    fn transaction_status_meta(&self) -> Option<&UiTransactionStatusMeta> {
        self.access().transaction_status_meta()
    }
}

/// A decoded v1 transaction and its RPC execution context.
pub struct DecodedV1Transaction<'a> {
    /// Notification signature, as in [`DecodedTransaction`].
    pub signature: Signature,
    /// Slot and optional transaction index.
    pub slot_index: SlotAndIndex,
    /// Complete v1 transaction, including inline resource configuration.
    pub transaction: V1Transaction,
    /// Execution metadata, required for CPI events.
    pub transaction_status_meta: &'a UiTransactionStatusMeta,
}

impl DecodedV1Transaction<'_> {
    /// Extract Anchor CPI events. V1 account addresses are all inline.
    pub fn extract_cpi_events(&self, filter: &CPIEventFilter) -> Result<CPIEvents, DecodeError> {
        extract_cpi_events(
            self.signature,
            self.slot_index,
            &self.transaction.message.account_keys,
            self.transaction_status_meta,
            filter,
        )
    }
}

impl crate::TransactionAccess for DecodedV1Transaction<'_> {
    fn slot(&self) -> Result<u64, DecodeError> {
        Ok(self.slot_index.0)
    }
    fn index(&self) -> Result<Option<usize>, DecodeError> {
        Ok(self.slot_index.1)
    }
    fn signature(&self) -> Result<&Signature, DecodeError> {
        Ok(&self.signature)
    }
    fn num_signers(&self, is_writable: bool) -> Result<usize, DecodeError> {
        let header = &self.transaction.message.header;
        let readonly = usize::from(header.num_readonly_signed_accounts);
        if is_writable {
            usize::from(header.num_required_signatures)
                .checked_sub(readonly)
                .ok_or_else(|| DecodeError::custom("invalid v1 signer header"))
        } else {
            Ok(readonly)
        }
    }
    fn num_accounts(&self) -> usize {
        self.transaction.message.account_keys.len()
    }
    fn message_signature(&self, idx: usize) -> Option<&Signature> {
        self.transaction.signatures.get(idx)
    }
    fn account_meta(&self, idx: usize) -> Result<Option<AccountMeta>, DecodeError> {
        let message = &self.transaction.message;
        let Some(pubkey) = message.account_keys.get(idx) else {
            return Ok(None);
        };
        let signed_end = usize::from(message.header.num_required_signatures);
        let unsigned_writable_end = self
            .num_accounts()
            .checked_sub(usize::from(message.header.num_readonly_unsigned_accounts))
            .filter(|end| *end >= signed_end)
            .ok_or_else(|| DecodeError::custom("invalid v1 account header"))?;
        let is_signer = idx < signed_end;
        let is_writable = if is_signer {
            idx < self.num_signers(true)?
        } else {
            idx < unsigned_writable_end
        };
        Ok(Some(AccountMeta {
            pubkey: *pubkey,
            is_signer,
            is_writable,
        }))
    }
    fn num_address_table_lookups(&self) -> usize {
        0
    }
    fn address_table_lookup(&self, _idx: usize) -> Option<&MessageAddressTableLookup> {
        None
    }
    fn num_instructions(&self) -> usize {
        self.transaction.message.instructions.len()
    }
    fn instruction(&self, idx: usize) -> Option<&CompiledInstruction> {
        self.transaction.message.instructions.get(idx)
    }
    fn transaction_status_meta(&self) -> Option<&UiTransactionStatusMeta> {
        Some(self.transaction_status_meta)
    }
}

impl crate::TransactionAccess for DecodedTransaction<'_> {
    fn slot(&self) -> Result<u64, DecodeError> {
        Ok(self.slot_index.0)
    }

    fn index(&self) -> Result<Option<usize>, DecodeError> {
        Ok(self.slot_index.1)
    }

    fn signature(&self) -> Result<&Signature, DecodeError> {
        Ok(&self.signature)
    }

    fn num_signers(&self, is_writable: bool) -> Result<usize, DecodeError> {
        let header = self.transaction.message.header();
        if is_writable {
            (header.num_required_signatures as usize)
                .checked_sub(self.num_signers(false)?)
                .ok_or_else(|| {
                    DecodeError::custom(
                        "invalid transaction message header: num_signed < num_readonly_signed",
                    )
                })
        } else {
            Ok(header.num_readonly_signed_accounts as usize)
        }
    }

    fn num_accounts(&self) -> usize {
        self.transaction.message.static_account_keys().len()
            + self.dynamic_writable_accounts.len()
            + self.dynamic_readonly_accounts.len()
    }

    fn message_signature(&self, idx: usize) -> Option<&Signature> {
        self.transaction.signatures.get(idx)
    }

    fn account_meta(&self, idx: usize) -> Result<Option<AccountMeta>, DecodeError> {
        let static_accounts = self.transaction.message.static_account_keys();
        let static_end = static_accounts.len();
        let dynamic_writable_length = self.dynamic_writable_accounts.len();
        let dynamic_readonly_length = self.dynamic_readonly_accounts.len();
        let dynamic_writable_end = static_end + dynamic_writable_length;
        let dynamic_end = dynamic_writable_end + dynamic_readonly_length;
        let meta = if idx >= dynamic_end {
            None
        } else if idx >= dynamic_writable_end {
            let idx = idx - dynamic_writable_end;
            Some(AccountMeta {
                pubkey: self.dynamic_readonly_accounts[idx],
                is_signer: false,
                is_writable: false,
            })
        } else if idx >= static_end {
            let idx = idx - static_end;
            Some(AccountMeta {
                pubkey: self.dynamic_writable_accounts[idx],
                is_signer: false,
                is_writable: true,
            })
        } else {
            let num_readonly_signed = self.num_signers(false)?;
            let num_readonly_unsigned = self
                .transaction
                .message
                .header()
                .num_readonly_unsigned_accounts as usize;
            let writable_signed_end = self.num_signers(true)?;
            let readonly_signed_end = writable_signed_end + num_readonly_signed;
            let writable_unsigend_end = static_end.checked_sub(num_readonly_unsigned).ok_or_else(|| {
               DecodeError::custom("invalid transaction message header: static_end < num_sigend + num_readonly_signed") 
            })?;
            let (is_signer, is_writable) = if idx >= writable_unsigend_end {
                (false, false)
            } else if idx >= readonly_signed_end {
                (false, true)
            } else if idx >= writable_signed_end {
                (true, false)
            } else {
                (true, true)
            };
            Some(AccountMeta {
                pubkey: static_accounts[idx],
                is_signer,
                is_writable,
            })
        };
        Ok(meta)
    }

    fn num_address_table_lookups(&self) -> usize {
        self.transaction
            .message
            .address_table_lookups()
            .map(|atls| atls.len())
            .unwrap_or_default()
    }

    fn address_table_lookup(&self, idx: usize) -> Option<&MessageAddressTableLookup> {
        self.transaction.message.address_table_lookups()?.get(idx)
    }

    fn num_instructions(&self) -> usize {
        self.transaction.message.instructions().len()
    }

    fn instruction(&self, idx: usize) -> Option<&CompiledInstruction> {
        self.transaction.message.instructions().get(idx)
    }

    fn transaction_status_meta(&self) -> Option<&UiTransactionStatusMeta> {
        Some(self.transaction_status_meta)
    }
}
