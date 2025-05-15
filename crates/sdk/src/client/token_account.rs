use std::ops::Deref;

use anchor_spl::associated_token::get_associated_token_address;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

/// Token Account Params.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenAccountParams {
    token: Option<Pubkey>,
    token_account: Option<Pubkey>,
}

impl TokenAccountParams {
    /// Set token account.
    pub fn set_token_account(&mut self, account: Pubkey) -> &mut Self {
        self.token_account = Some(account);
        self
    }

    /// Set token.
    pub fn set_token(&mut self, mint: Pubkey) -> &mut Self {
        self.token = Some(mint);
        self
    }

    /// Get token.
    pub fn token(&self) -> Option<&Pubkey> {
        self.token.as_ref()
    }

    /// Get or find associated token account.
    pub fn get_or_find_associated_token_account(&self, owner: Option<&Pubkey>) -> Option<Pubkey> {
        match self.token_account {
            Some(account) => Some(account),
            None => {
                let token = self.token.as_ref()?;
                let owner = owner?;
                Some(get_associated_token_address(owner, token))
            }
        }
    }

    /// Get of fetch token and token account.
    ///
    /// Returns `(token, token_account)` if success.
    pub async fn get_or_fetch_token_and_token_account<S, C>(
        &self,
        client: &crate::Client<C>,
        owner: Option<&Pubkey>,
    ) -> crate::Result<Option<(Pubkey, Pubkey)>>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        use anchor_spl::token::TokenAccount;
        match (self.token, self.token_account) {
            (Some(token), Some(account)) => Ok(Some((token, account))),
            (None, Some(account)) => {
                let mint = client
                    .account::<TokenAccount>(&account)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .mint;
                Ok(Some((mint, account)))
            }
            (Some(token), None) => {
                let Some(account) = self.get_or_find_associated_token_account(owner) else {
                    return Err(crate::Error::custom(
                        "cannot find associated token account: `owner` is not provided",
                    ));
                };
                Ok(Some((token, account)))
            }
            (None, None) => Ok(None),
        }
    }

    /// Returns whether the params is empty.
    pub fn is_empty(&self) -> bool {
        self.token.is_none() && self.token.is_none()
    }
}
