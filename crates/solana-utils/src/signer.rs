use std::{collections::HashMap, fmt, ops::Deref, sync::Arc};

use dyn_clone::{clone_trait_object, DynClone};
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::Signature,
    signer::{Signer, SignerError},
    transaction::VersionedTransaction,
};

use crate::{
    address_lookup_table::AddressLookupTables, instruction_group::GetInstructionsOptions,
    AtomicGroup,
};

/// Boxed Signer.
pub type LocalBoxSigner = Box<dyn Signer>;

/// Boxed Clonable Signer.
#[derive(Clone)]
pub struct BoxClonableSigner<'a>(pub Box<dyn CloneableSigner + 'a>);

impl fmt::Debug for BoxClonableSigner<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxClonableSigner")
            .field("pubkey", &self.0.pubkey())
            .finish_non_exhaustive()
    }
}

impl<'a> BoxClonableSigner<'a> {
    /// Create from `impl Signer`.
    pub fn new(signer: impl Signer + Clone + 'a) -> Self {
        Self(Box::new(signer))
    }
}

/// Clonable Signer.
pub trait CloneableSigner: Signer + DynClone {}

impl<T: Signer + Clone> CloneableSigner for T {}

clone_trait_object!(CloneableSigner);

impl Signer for BoxClonableSigner<'_> {
    fn pubkey(&self) -> Pubkey {
        self.0.pubkey()
    }

    fn sign_message(&self, message: &[u8]) -> Signature {
        self.0.sign_message(message)
    }

    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        self.0.try_pubkey()
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        self.0.try_sign_message(message)
    }

    fn is_interactive(&self) -> bool {
        self.0.is_interactive()
    }
}

/// Boxed signer.
pub struct BoxSigner(Box<dyn Signer + Send + Sync>);

impl Signer for BoxSigner {
    fn pubkey(&self) -> Pubkey {
        self.0.pubkey()
    }

    fn try_pubkey(&self) -> Result<Pubkey, solana_sdk::signer::SignerError> {
        self.0.try_pubkey()
    }

    fn sign_message(&self, message: &[u8]) -> solana_sdk::signature::Signature {
        self.0.sign_message(message)
    }

    fn try_sign_message(
        &self,
        message: &[u8],
    ) -> Result<solana_sdk::signature::Signature, solana_sdk::signer::SignerError> {
        self.0.try_sign_message(message)
    }

    fn is_interactive(&self) -> bool {
        self.0.is_interactive()
    }
}

/// Shared Signer.
pub type SignerRef = Arc<BoxSigner>;

/// Create a new shared signer.
pub fn shared_signer(signer: impl Signer + Send + Sync + 'static) -> SignerRef {
    SignerRef::new(BoxSigner(Box::new(signer)))
}

/// Transaction Signers.
#[derive(Debug, Clone)]
pub struct TransactionSigners<C> {
    signers: HashMap<Pubkey, C>,
}

impl<C> Default for TransactionSigners<C> {
    fn default() -> Self {
        Self {
            signers: Default::default(),
        }
    }
}

impl<C> TransactionSigners<C> {
    fn project(&self, ag: &AtomicGroup) -> HashMap<Pubkey, &C> {
        ag.external_signers()
            .filter_map(|pubkey| self.signers.get(pubkey).map(|s| (*pubkey, s)))
            .collect()
    }
}

impl<C: Deref<Target = impl Signer>> TransactionSigners<C> {
    /// Insert a signer.
    pub fn insert(&mut self, signer: C) -> Option<C> {
        self.signers.insert(signer.pubkey(), signer)
    }

    /// Sign the given [`AtomicGroup`].
    pub fn sign_atomic_instruction_group(
        &self,
        ag: &AtomicGroup,
        recent_blockhash: Hash,
        options: GetInstructionsOptions,
        luts: Option<&AddressLookupTables>,
        allow_partial_sign: bool,
    ) -> crate::Result<VersionedTransaction> {
        let signers = self.project(ag);
        let mut tx = ag.partially_signed_transaction_with_blockhash_and_options(
            recent_blockhash,
            options,
            luts,
        )?;
        let message = tx.message.serialize();
        let expected_signers = &tx.message.static_account_keys()
            [0..(tx.message.header().num_required_signatures as usize)];
        let default_signature = Signature::default();
        for (idx, signature) in tx.signatures.iter_mut().enumerate() {
            if *signature == default_signature {
                let pubkey = expected_signers[idx];
                let Some(signer) = signers.get(&pubkey) else {
                    if allow_partial_sign {
                        continue;
                    } else {
                        return Err(crate::Error::Signer(SignerError::Custom(format!(
                            "missing signer for {pubkey}"
                        ))));
                    }
                };
                *signature = signer.sign_message(&message);
            }
        }
        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer>> FromIterator<C> for TransactionSigners<C> {
    fn from_iter<T: IntoIterator<Item = C>>(iter: T) -> Self {
        let mut this = Self::default();
        for signer in iter {
            this.insert(signer);
        }
        this
    }
}

impl<C: Deref<Target = impl Signer>> Extend<C> for TransactionSigners<C> {
    fn extend<T: IntoIterator<Item = C>>(&mut self, iter: T) {
        for signer in iter {
            self.insert(signer);
        }
    }
}
