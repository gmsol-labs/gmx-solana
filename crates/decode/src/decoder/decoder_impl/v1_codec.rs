//! SIMD-0385 wire decoding using Solana 2.1 types. No signing or execution checks.
//!
//! Format/validation reference: anza-xyz/solana-sdk, message 4.4.0
//! (aa9ce86aedecee08f1f61bc1bb0c1e2f90f55de7), transaction 4.1.5
//! (3a4e8ef7dd15655296ecea4b0caa3d7bbc859335). See fixtures/README.md.

use solana_sdk::{
    hash::Hash, instruction::CompiledInstruction, message::MessageHeader, pubkey::Pubkey,
    signature::Signature,
};

use crate::DecodeError;

pub(super) const PREFIX: u8 = 0x81;
const MAX_TRANSACTION_SIZE: usize = 4096;

/// Inline v1 resource configuration. Absence is preserved, not replaced by defaults.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct V1TransactionConfig {
    /// Total priority fee in lamports (not a per-compute-unit price).
    pub priority_fee: Option<u64>,
    /// Requested compute unit limit.
    pub compute_unit_limit: Option<u32>,
    /// Requested loaded account data limit in bytes.
    pub loaded_accounts_data_size_limit: Option<u32>,
    /// Requested heap size in bytes.
    pub heap_size: Option<u32>,
}

/// Complete v1 message, with inline accounts and no address lookup tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V1Message {
    /// Signer and read-only account counts, using the existing account ordering.
    pub header: MessageHeader,
    /// Inline resource configuration.
    pub config: V1TransactionConfig,
    /// Transaction lifetime specifier, currently a blockhash.
    pub lifetime_specifier: Hash,
    /// All message account addresses, in wire order.
    pub account_keys: Vec<Pubkey>,
    /// Top-level compiled instructions, in execution order.
    pub instructions: Vec<CompiledInstruction>,
}

/// Complete v1 transaction. Wire signatures follow the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V1Transaction {
    /// Signatures in signer order. Decoding does not verify their authenticity.
    pub signatures: Vec<Signature>,
    /// Full v1 message.
    pub message: V1Message,
}

pub(super) fn decode(bytes: &[u8]) -> Result<V1Transaction, DecodeError> {
    if bytes.len() > MAX_TRANSACTION_SIZE {
        return Err(DecodeError::custom("v1 transaction exceeds 4096 bytes"));
    }
    let mut reader = Reader(bytes);
    if reader.byte()? != PREFIX {
        return Err(DecodeError::custom("invalid v1 transaction prefix"));
    }
    let header = MessageHeader {
        num_required_signatures: reader.byte()?,
        num_readonly_signed_accounts: reader.byte()?,
        num_readonly_unsigned_accounts: reader.byte()?,
    };
    let mask = u32::from_le_bytes(reader.array()?);
    // Priority fee occupies a pair of bits; either both or neither must be set.
    if mask & !0x1f != 0 || matches!(mask & 3, 1 | 2) {
        return Err(DecodeError::custom("invalid v1 config mask"));
    }
    let lifetime_specifier = Hash::new_from_array(reader.array()?);
    let num_instructions = usize::from(reader.byte()?);
    let num_accounts = usize::from(reader.byte()?);
    let num_signatures = usize::from(header.num_required_signatures);
    if num_signatures > 12 || num_accounts > 64 || num_instructions > 64 {
        return Err(DecodeError::custom("v1 count exceeds protocol limit"));
    }
    if header.num_readonly_signed_accounts >= header.num_required_signatures
        || num_accounts < num_signatures + usize::from(header.num_readonly_unsigned_accounts)
    {
        return Err(DecodeError::custom("invalid v1 account header"));
    }
    let mut account_keys = Vec::with_capacity(num_accounts);
    for _ in 0..num_accounts {
        let key = Pubkey::new_from_array(reader.array()?);
        // At most 64 addresses; avoid allocating an additional set.
        if account_keys.contains(&key) {
            return Err(DecodeError::custom("duplicate v1 account address"));
        }
        account_keys.push(key);
    }
    let config = V1TransactionConfig {
        priority_fee: if mask & 3 != 0 {
            Some(u64::from_le_bytes(reader.array()?))
        } else {
            None
        },
        compute_unit_limit: if mask & 4 != 0 {
            Some(u32::from_le_bytes(reader.array()?))
        } else {
            None
        },
        loaded_accounts_data_size_limit: if mask & 8 != 0 {
            Some(u32::from_le_bytes(reader.array()?))
        } else {
            None
        },
        heap_size: if mask & 16 != 0 {
            Some(u32::from_le_bytes(reader.array()?))
        } else {
            None
        },
    };
    if let Some(heap) = config.heap_size {
        if !(32 * 1024..=256 * 1024).contains(&heap) || heap % 1024 != 0 {
            return Err(DecodeError::custom("invalid v1 heap size"));
        }
    }

    // All instruction headers precede all instruction payloads (not interleaved).
    let headers = reader.take(num_instructions * 4)?;
    let mut instructions = Vec::with_capacity(num_instructions);
    for ix in headers.chunks_exact(4) {
        let program_id_index = ix[0];
        if program_id_index == 0 || usize::from(program_id_index) >= num_accounts {
            return Err(DecodeError::custom("invalid v1 program index"));
        }
        let accounts = reader.take(usize::from(ix[1]))?;
        if accounts
            .iter()
            .any(|index| usize::from(*index) >= num_accounts)
        {
            return Err(DecodeError::custom("invalid v1 instruction account index"));
        }
        let data = reader.take(usize::from(u16::from_le_bytes([ix[2], ix[3]])))?;
        instructions.push(CompiledInstruction {
            program_id_index,
            accounts: accounts.to_vec(),
            data: data.to_vec(),
        });
    }
    let mut signatures = Vec::with_capacity(num_signatures);
    for _ in 0..num_signatures {
        signatures.push(Signature::from(reader.array::<64>()?));
    }
    if !reader.0.is_empty() {
        return Err(DecodeError::custom("trailing bytes after v1 transaction"));
    }
    Ok(V1Transaction {
        signatures,
        message: V1Message {
            header,
            config,
            lifetime_specifier,
            account_keys,
            instructions,
        },
    })
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], DecodeError> {
        let (value, rest) = self
            .0
            .split_at_checked(count)
            .ok_or_else(|| DecodeError::custom("truncated v1 transaction"))?;
        self.0 = rest;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        self.take(N)?.try_into().map_err(DecodeError::custom)
    }

    fn byte(&mut self) -> Result<u8, DecodeError> {
        Ok(self.array::<1>()?[0])
    }
}
