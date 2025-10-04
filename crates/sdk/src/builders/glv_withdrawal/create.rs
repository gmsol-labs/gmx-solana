use anchor_lang::prelude::AccountMeta;
use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use gmsol_model::num_traits::Zero;
use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::CreateWithdrawalParams,
};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, AtomicGroup, IntoAtomicGroup, ProgramExt,
};
use solana_sdk::system_program;
use typed_builder::TypedBuilder;

use crate::{
    builders::{
        deposit::MIN_EXECUTION_LAMPORTS_FOR_DEPOSIT,
        utils::{generate_nonce, prepare_ata},
        MarketTokenIxBuilder, NonceBytes, PoolTokenHint, StoreProgram, StoreProgramIxBuilder,
    },
    serde::StringPubkey,
};

/// Builder for the `create_withdrawal` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateWithdrawal {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Reciever.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option, into))]
    pub receiver: Option<StringPubkey>,
    /// Nonce for the deposit.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option, into))]
    pub nonce: Option<NonceBytes>,
    /// Execution fee paid to the keeper in lamports.
    #[cfg_attr(serde, serde(default = "default_execution_lamports"))]
    #[builder(default = MIN_EXECUTION_LAMPORTS_FOR_DEPOSIT)]
    pub execution_lamports: u64,
    /// The market token of the market in which the deposit will be created.
    #[builder(setter(into))]
    pub market_token: StringPubkey,
    /// Market token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub market_token_account: Option<StringPubkey>,
    /// Long receive token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub long_receive_token: Option<StringPubkey>,
    /// Swap path for long receive token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub long_swap_path: Vec<StringPubkey>,
    /// Short receive token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub short_receive_token: Option<StringPubkey>,
    /// Swap path for short receive token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub short_swap_path: Vec<StringPubkey>,
    /// market token amount.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub market_token_amount: u64,
    /// Minimum amount of long receive tokens.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub min_long_receive_amount: u64,
    /// Minimum amount of short receive tokens.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub min_short_receive_amount: u64,
    /// Whether to unwrap the native token when receiving (e.g., convert WSOL to SOL).
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub unwrap_native_on_receive: bool,
    /// Whether to skip the creation of long receive token ATA.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub skip_long_receive_token_ata_creation: bool,
    /// Whether to skip the creation of short receive token ATA.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub skip_short_receive_token_ata_creation: bool,
}

#[cfg(serde)]
fn default_execution_lamports() -> u64 {
    MIN_EXECUTION_LAMPORTS_FOR_DEPOSIT
}

impl StoreProgramIxBuilder for CreateWithdrawal {
    fn store_program(&self) -> &StoreProgram {
        &self.program
    }
}

impl MarketTokenIxBuilder for CreateWithdrawal {
    fn market_token(&self) -> &anchor_lang::prelude::Pubkey {
        &self.market_token
    }
}

impl IntoAtomicGroup for CreateWithdrawal {
    type Hint = CreateWithdrawalHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        if self.market_token_amount.is_zero() {
            return Err(gmsol_solana_utils::Error::custom(
                "invalid argument: empty withdrawal",
            ));
        }

        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let receiver = self.receiver.as_deref().copied().unwrap_or(owner);
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        let withdrawal = self.program.find_withdrawal_address(&owner, &nonce);
        let token_program_id = anchor_spl::token::ID;
        let market_token = self.market_token.0;

        let long_receive_token = self
            .long_receive_token
            .as_deref()
            .unwrap_or(&hint.pool_tokens.long_token);
        let short_receive_token = self
            .short_receive_token
            .as_deref()
            .unwrap_or(&hint.pool_tokens.short_token);

        let (long_receive_token_escrow, prepare) = prepare_ata(
            &owner,
            &withdrawal,
            Some(long_receive_token),
            &token_program_id,
        )
        .expect("must exist");
        insts.add(prepare);

        let (short_receive_token_escrow, prepare) = prepare_ata(
            &owner,
            &withdrawal,
            Some(short_receive_token),
            &token_program_id,
        )
        .expect("must exist");
        insts.add(prepare);

        let (market_token_escrow, prepare) =
            prepare_ata(&owner, &withdrawal, Some(&market_token), &token_program_id)
                .expect("must exist");
        insts.add(prepare);

        let market_token_account = self
            .market_token_account
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                get_associated_token_address_with_program_id(
                    &owner,
                    &market_token,
                    &token_program_id,
                )
            });

        let (_long_receive_token_ata, prepare) = prepare_ata(
            &owner,
            &receiver,
            Some(long_receive_token),
            &token_program_id,
        )
        .expect("must exist");
        if !self.skip_long_receive_token_ata_creation {
            insts.add(prepare);
        }

        let (_short_receive_token_ata, prepare) = prepare_ata(
            &owner,
            &receiver,
            Some(short_receive_token),
            &token_program_id,
        )
        .expect("must exist");
        if !self.skip_short_receive_token_ata_creation {
            insts.add(prepare);
        }

        let params = CreateWithdrawalParams {
            execution_lamports: self.execution_lamports,
            should_unwrap_native_token: self.unwrap_native_on_receive,
            long_token_swap_path_length: self
                .long_swap_path
                .len()
                .try_into()
                .map_err(gmsol_solana_utils::Error::custom)?,
            short_token_swap_path_length: self
                .short_swap_path
                .len()
                .try_into()
                .map_err(gmsol_solana_utils::Error::custom)?,
            market_token_amount: self.market_token_amount,
            min_long_token_amount: self.min_long_receive_amount,
            min_short_token_amount: self.min_short_receive_amount,
        };

        let create =
            self.program
                .anchor_instruction(args::CreateWithdrawal {
                    nonce: nonce.to_bytes(),
                    params,
                })
                .anchor_accounts(
                    accounts::CreateWithdrawal {
                        owner,
                        receiver,
                        store: self.program.store.0,
                        market: self.program.find_market_address(&market_token),
                        withdrawal,
                        market_token,
                        market_token_escrow,
                        final_long_token: *long_receive_token,
                        final_short_token: *short_receive_token,
                        final_long_token_escrow: long_receive_token_escrow,
                        final_short_token_escrow: short_receive_token_escrow,
                        market_token_source: market_token_account,
                        system_program: system_program::ID,
                        token_program: token_program_id,
                        associated_token_program: associated_token::ID,
                    },
                    true,
                )
                .accounts(
                    self.long_swap_path
                        .iter()
                        .enumerate()
                        .map(|(idx, mint)| AccountMeta {
                            pubkey: self.program.find_market_address(mint),
                            is_signer: false,
                            is_writable: idx == 0,
                        })
                        .chain(self.short_swap_path.iter().enumerate().map(|(idx, mint)| {
                            AccountMeta {
                                pubkey: self.program.find_market_address(mint),
                                is_signer: false,
                                is_writable: idx == 0,
                            }
                        }))
                        .collect::<Vec<_>>(),
                )
                .build();
        insts.add(create);

        Ok(insts)
    }
}

/// Hint for [`CreateWithdrawal`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateWithdrawalHint {
    /// Pool tokens.
    #[builder(setter(into))]
    pub pool_tokens: PoolTokenHint,
}

impl FromRpcClientWith<CreateWithdrawal> for CreateWithdrawalHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a CreateWithdrawal,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        let pool_tokens = PoolTokenHint::from_rpc_client_with(builder, client).await?;
        Ok(Self { pool_tokens })
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    use gmsol_solana_utils::{
        client_traits::GenericRpcClient, cluster::Cluster, transaction_builder::default_before_sign,
    };
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn create_withdrawal() -> crate::Result<()> {
        let long_token = Pubkey::new_unique();
        let short_token = Pubkey::new_unique();
        CreateWithdrawal::builder()
            .payer(Pubkey::new_unique())
            .long_swap_path([Pubkey::new_unique().into()])
            .market_token_amount(1_000_000_000)
            .long_receive_token(Some(Pubkey::new_unique().into()))
            .market_token(Pubkey::new_unique())
            .unwrap_native_on_receive(true)
            .build()
            .into_atomic_group(
                &CreateWithdrawalHint::builder()
                    .pool_tokens(
                        PoolTokenHint::builder()
                            .long_token(long_token)
                            .short_token(short_token)
                            .build(),
                    )
                    .build(),
            )?
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
                default_before_sign,
            )?;
        Ok(())
    }

    #[async_test]
    async fn create_withdrawal_with_rpc() -> crate::Result<()> {
        let market_token: Pubkey = "5sdFW7wrKsxxYHMXoqDmNHkGyCWsbLEFb1x1gzBBm4Hx".parse()?;
        let wsol: Pubkey = "So11111111111111111111111111111111111111112".parse()?;

        let cluster = Cluster::Devnet;
        let client = GenericRpcClient::new(cluster.url());

        CreateWithdrawal::builder()
            .payer(Pubkey::new_unique())
            .short_swap_path([Pubkey::new_unique().into()])
            .short_receive_token(Some(wsol.into()))
            .market_token_amount(1_000_000_000)
            .market_token(market_token)
            .unwrap_native_on_receive(true)
            .build()
            .into_atomic_group_with_rpc_client(&client)
            .await?
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
                default_before_sign,
            )?;

        Ok(())
    }
}
