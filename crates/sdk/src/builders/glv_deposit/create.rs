use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use gmsol_model::num_traits::Zero;
use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::CreateGlvDepositParams,
};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, AtomicGroup, IntoAtomicGroup, ProgramExt,
};
use solana_sdk::{instruction::AccountMeta, system_program};
use typed_builder::TypedBuilder;

use crate::{
    builders::{
        glv_deposit::MIN_EXECUTION_LAMPORTS_FOR_GLV_DEPOSIT,
        utils::{generate_nonce, prepare_ata},
        MarketTokenIxBuilder, NonceBytes, PoolTokenHint, StoreProgram, StoreProgramIxBuilder,
    },
    serde::StringPubkey,
};

/// Builder for the `create_glv_deposit` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateGlvDeposit {
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
    /// The GLV token.
    #[builder(setter(into))]
    pub glv_token: StringPubkey,
    /// The market token of the market in which the deposit will be created.
    #[builder(setter(into))]
    pub market_token: StringPubkey,
    /// Execution fee paid to the keeper in lamports.
    #[cfg_attr(serde, serde(default = "default_execution_lamports"))]
    #[builder(default = MIN_EXECUTION_LAMPORTS_FOR_GLV_DEPOSIT)]
    pub execution_lamports: u64,
    /// Long pay token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub long_pay_token: Option<StringPubkey>,
    /// Long pay token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub long_pay_token_account: Option<StringPubkey>,
    /// Swap path for long pay token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub long_swap_path: Vec<StringPubkey>,
    /// Short pay token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub short_pay_token: Option<StringPubkey>,
    /// Short pay token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub short_pay_token_account: Option<StringPubkey>,
    /// Swap path for short pay token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub short_swap_path: Vec<StringPubkey>,
    /// Market token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub market_token_account: Option<StringPubkey>,
    /// Long pay token amount.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub long_pay_amount: u64,
    /// Short pay token amount.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub short_pay_amount: u64,
    /// Market token amount to pay.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub market_token_amount: u64,
    /// Minimum amount of output market tokens.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub min_market_token_amount: u64,
    /// Minimum amount of output GLV tokens.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub min_receive_amount: u64,
    /// Whether to unwrap the native token when receiving (e.g., convert WSOL to SOL).
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub unwrap_native_on_receive: bool,
    /// Whether to skip the creation of GLV token ATA.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub skip_glv_token_ata_creation: bool,
}

#[cfg(serde)]
fn default_execution_lamports() -> u64 {
    MIN_EXECUTION_LAMPORTS_FOR_GLV_DEPOSIT
}

impl StoreProgramIxBuilder for CreateGlvDeposit {
    fn store_program(&self) -> &StoreProgram {
        &self.program
    }
}

impl MarketTokenIxBuilder for CreateGlvDeposit {
    fn market_token(&self) -> &anchor_lang::prelude::Pubkey {
        &self.market_token
    }
}

impl IntoAtomicGroup for CreateGlvDeposit {
    type Hint = CreateGlvDepositHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        if self.long_pay_amount.is_zero()
            && self.short_pay_amount.is_zero()
            && self.market_token_amount.is_zero()
        {
            return Err(gmsol_solana_utils::Error::custom(
                "invalid argument: empty GLV deposit",
            ));
        }

        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let receiver = self.receiver.as_deref().copied().unwrap_or(owner);
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        let glv_deposit = self.program.find_glv_deposit_address(&owner, &nonce);
        let token_program_id = anchor_spl::token::ID;
        let glv_token_program_id = anchor_spl::token_2022::ID;
        let market_token = self.market_token.0;
        let glv_token = self.glv_token.0;

        let long_pay_token = (!self.long_pay_amount.is_zero()).then(|| {
            self.long_pay_token
                .as_deref()
                .unwrap_or(&hint.pool_tokens.long_token)
        });
        let long_pay_token_account = long_pay_token.as_ref().map(|token| {
            self.long_pay_token_account
                .as_deref()
                .copied()
                .unwrap_or_else(|| {
                    get_associated_token_address_with_program_id(&owner, token, &token_program_id)
                })
        });
        let short_pay_token = (!self.short_pay_amount.is_zero()).then(|| {
            self.short_pay_token
                .as_deref()
                .unwrap_or(&hint.pool_tokens.short_token)
        });
        let short_pay_token_account = short_pay_token.as_ref().map(|token| {
            self.short_pay_token_account
                .as_deref()
                .copied()
                .unwrap_or_else(|| {
                    get_associated_token_address_with_program_id(&owner, token, &token_program_id)
                })
        });
        let market_token_account = (!self.market_token_amount.is_zero()).then(|| {
            self.market_token_account
                .as_deref()
                .copied()
                .unwrap_or_else(|| {
                    get_associated_token_address_with_program_id(
                        &owner,
                        &market_token,
                        &token_program_id,
                    )
                })
        });

        let (long_pay_token_escrow, prepare) =
            prepare_ata(&owner, &glv_deposit, long_pay_token, &token_program_id).unzip();
        insts.extend(prepare);

        let (short_pay_token_escrow, prepare) =
            prepare_ata(&owner, &glv_deposit, short_pay_token, &token_program_id).unzip();
        insts.extend(prepare);

        let (market_token_escrow, prepare) =
            prepare_ata(&owner, &glv_deposit, Some(&market_token), &token_program_id)
                .expect("must exist");
        insts.add(prepare);

        let (glv_token_escrow, prepare) = prepare_ata(
            &owner,
            &glv_deposit,
            Some(&glv_token),
            &glv_token_program_id,
        )
        .expect("must exist");
        insts.add(prepare);

        let (_glv_token_ata, prepare) =
            prepare_ata(&owner, &receiver, Some(&glv_token), &glv_token_program_id)
                .expect("must exist");
        if !self.skip_glv_token_ata_creation {
            insts.add(prepare);
        }

        let params = CreateGlvDepositParams {
            execution_lamports: self.execution_lamports,
            long_token_swap_length: self
                .long_swap_path
                .len()
                .try_into()
                .map_err(gmsol_solana_utils::Error::custom)?,
            short_token_swap_length: self
                .short_swap_path
                .len()
                .try_into()
                .map_err(gmsol_solana_utils::Error::custom)?,
            initial_long_token_amount: self.long_pay_amount,
            initial_short_token_amount: self.short_pay_amount,
            market_token_amount: self.market_token_amount,
            min_market_token_amount: self.min_market_token_amount,
            min_glv_token_amount: self.min_receive_amount,
            should_unwrap_native_token: self.unwrap_native_on_receive,
        };

        let create =
            self.program
                .anchor_instruction(args::CreateGlvDeposit {
                    nonce: nonce.to_bytes(),
                    params,
                })
                .anchor_accounts(
                    accounts::CreateGlvDeposit {
                        owner,
                        receiver,
                        store: self.program.store.0,
                        glv: self.program.find_glv_address(&glv_token),
                        market: self.program.find_market_address(&market_token),
                        glv_deposit,
                        glv_token,
                        market_token,
                        initial_long_token: long_pay_token.copied(),
                        initial_short_token: short_pay_token.copied(),
                        glv_token_escrow,
                        market_token_escrow,
                        initial_long_token_escrow: long_pay_token_escrow,
                        initial_short_token_escrow: short_pay_token_escrow,
                        initial_long_token_source: long_pay_token_account,
                        initial_short_token_source: short_pay_token_account,
                        market_token_source: market_token_account,
                        system_program: system_program::ID,
                        token_program: token_program_id,
                        glv_token_program: glv_token_program_id,
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

/// Hint for [`CreateGlvDeposit`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateGlvDepositHint {
    /// Pool tokens.
    #[builder(setter(into))]
    pub pool_tokens: PoolTokenHint,
}

impl FromRpcClientWith<CreateGlvDeposit> for CreateGlvDepositHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a CreateGlvDeposit,
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
    fn create_glv_deposit() -> crate::Result<()> {
        let long_token = Pubkey::new_unique();
        let short_token = Pubkey::new_unique();
        CreateGlvDeposit::builder()
            .payer(Pubkey::new_unique())
            .long_swap_path([Pubkey::new_unique().into()])
            .long_pay_amount(1_000_000_000)
            .long_pay_token(Some(Pubkey::new_unique().into()))
            .glv_token(Pubkey::new_unique())
            .market_token(Pubkey::new_unique())
            .unwrap_native_on_receive(true)
            .build()
            .into_atomic_group(
                &CreateGlvDepositHint::builder()
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
    async fn create_glv_deposit_with_rpc() -> crate::Result<()> {
        let market_token: Pubkey = "5sdFW7wrKsxxYHMXoqDmNHkGyCWsbLEFb1x1gzBBm4Hx".parse()?;
        let wsol: Pubkey = "So11111111111111111111111111111111111111112".parse()?;

        let cluster = Cluster::Devnet;
        let client = GenericRpcClient::new(cluster.url());

        CreateGlvDeposit::builder()
            .payer(Pubkey::new_unique())
            .short_swap_path([Pubkey::new_unique().into()])
            .short_pay_amount(1_000_000_000)
            .short_pay_token(Some(wsol.into()))
            .glv_token(Pubkey::new_unique())
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
