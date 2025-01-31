use std::ops::Deref;

use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{accounts, instruction};

use crate::store::token::TokenAccountOps;

/// Claim fees builder.
// TODO: implement this.
#[allow(dead_code)]
pub struct ClaimFeesBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    market_token: Pubkey,
    is_long_token: bool,
    hint_token: Option<Pubkey>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ClaimFeesBuilder<'a, C> {
    /// Create a new builder.
    pub fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long_token: bool,
    ) -> Self {
        Self {
            client,
            store: *store,
            market_token: *market_token,
            is_long_token,
            hint_token: None,
        }
    }

    /// Set hint.
    pub fn set_hint(&mut self, token: Pubkey) -> &mut Self {
        self.hint_token = Some(token);
        self
    }

    /// Build.
    pub async fn build(&self) -> crate::Result<TransactionBuilder<'a, C>> {
        let market = self
            .client
            .find_market_address(&self.store, &self.market_token);
        let token = match self.hint_token {
            Some(token) => token,
            None => {
                let market = self.client.market(&market).await?;
                market.meta().pnl_token(self.is_long_token)
            }
        };

        let authority = self.client.payer();
        let vault = self.client.find_market_vault_address(&self.store, &token);
        // FIXME: read program id from the market.
        let token_program = anchor_spl::token::ID;
        let target =
            get_associated_token_address_with_program_id(&authority, &token, &token_program);

        let prepare = self
            .client
            .prepare_associated_token_account(&token, &token_program, None);

        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::ClaimFeesFromMarket {
                authority,
                store: self.store,
                market,
                token_mint: token,
                vault,
                target,
                token_program,
                event_authority: self.client.store_event_authority(),
                program: *self.client.store_program_id(),
            })
            .anchor_args(instruction::ClaimFeesFromMarket {});

        Ok(prepare.merge(rpc))
    }
}
