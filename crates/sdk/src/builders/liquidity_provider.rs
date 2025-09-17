use std::{collections::BTreeSet, num::NonZeroU64};

use anchor_lang::system_program;
use gmsol_programs::{
    gmsol_liquidity_provider::client::{accounts, args},
    gmsol_store::accounts::{Glv, Market, Store},
};
use gmsol_solana_utils::{
    client_traits::{FromRpcClientWith, RpcClientExt},
    AtomicGroup, IntoAtomicGroup, Program, ProgramExt,
};
use gmsol_utils::{
    oracle::PriceProviderKind,
    pubkey::optional_address,
    swap::SwapActionParams,
    token_config::{token_records, TokensWithFeed},
};
use rand::Rng;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use typed_builder::TypedBuilder;

use crate::{
    serde::{
        serde_price_feed::{to_tokens_with_feeds, SerdeTokenRecord},
        StringPubkey,
    },
    utils::{
        glv::split_to_accounts,
        market::ordered_tokens,
        token_map::{FeedAddressMap, FeedsParser, TokenMap},
        zero_copy::ZeroCopy,
    },
};

use super::StoreProgram;

/// A liquidity-provider program.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct LiquidityProviderProgram {
    /// Program ID.
    #[builder(setter(into))]
    pub id: StringPubkey,
}

impl Default for LiquidityProviderProgram {
    fn default() -> Self {
        Self {
            id: <Self as anchor_lang::Id>::id().into(),
        }
    }
}

impl anchor_lang::Id for LiquidityProviderProgram {
    fn id() -> Pubkey {
        gmsol_programs::gmsol_liquidity_provider::ID
    }
}

impl Program for LiquidityProviderProgram {
    fn id(&self) -> &Pubkey {
        &self.id
    }
}

impl LiquidityProviderProgram {
    /// Find PDA for global state account.
    pub fn find_global_state_address(&self) -> Pubkey {
        crate::pda::find_lp_global_state_address(&self.id).0
    }

    /// Find PDA for stake position account.
    pub fn find_stake_position_address(
        &self,
        owner: &Pubkey,
        position_id: u64,
        controller: &Pubkey,
    ) -> Pubkey {
        crate::pda::find_lp_stake_position_address(owner, position_id, controller, &self.id).0
    }

    /// Find PDA for stake position vault.
    pub fn find_stake_position_vault_address(&self, position: &Pubkey) -> Pubkey {
        crate::pda::find_lp_stake_position_vault_address(position, &self.id).0
    }

    /// Find PDA for LP token controller account.
    pub fn find_lp_token_controller_address(
        &self,
        global_state: &Pubkey,
        lp_token_mint: &Pubkey,
        controller_index: u64,
    ) -> Pubkey {
        crate::pda::find_lp_token_controller_address(
            global_state,
            lp_token_mint,
            controller_index,
            &self.id,
        )
        .0
    }
}

/// Builder for LP token staking instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct StakeLpToken {
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Oracle buffer account.
    #[builder(setter(into))]
    pub oracle: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token kind.
    pub lp_token_kind: LpTokenKind,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// LP token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub lp_token_account: Option<StringPubkey>,
    /// Position ID.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub position_id: Option<u64>,
    /// Stake amount.
    pub amount: NonZeroU64,
    /// Feeds Parser.
    #[cfg_attr(serde, serde(skip))]
    #[builder(default)]
    pub feeds_parser: FeedsParser,
}

impl StakeLpToken {
    /// Insert a feed parser.
    pub fn insert_feed_parser(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.feeds_parser
            .insert_pull_oracle_feed_parser(provider, map);
        Ok(())
    }

    /// Set a specific position ID instead of using random generation.
    pub fn with_position_id(mut self, position_id: u64) -> Self {
        self.position_id = Some(position_id);
        self
    }

    fn position_id(&self) -> u64 {
        self.position_id.unwrap_or_else(|| rand::thread_rng().gen())
    }

    fn lp_token_account(&self, token_program_id: &Pubkey) -> Pubkey {
        self.lp_token_account
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                anchor_spl::associated_token::get_associated_token_address_with_program_id(
                    &self.payer,
                    &self.lp_token_mint,
                    token_program_id,
                )
            })
    }

    fn shared_args(&self) -> SharedArgs {
        let owner = self.payer.0;
        let position_id = self.position_id();
        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller =
            self.lp_program
                .find_lp_token_controller_address(&global_state, &lp_mint, 0);

        let position =
            self.lp_program
                .find_stake_position_address(&owner, position_id, &controller);
        let position_vault = self.lp_program.find_stake_position_vault_address(&position);

        SharedArgs {
            owner,
            position_id,
            global_state,
            lp_mint,
            position,
            position_vault,
            gt_store: self.store_program.store.0,
            gt_program: *self.store_program.id(),
        }
    }

    fn feeds(&self, hint: &StakeLpTokenHint) -> gmsol_solana_utils::Result<Vec<AccountMeta>> {
        self.feeds_parser
            .parse(&hint.to_tokens_with_feeds()?)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)
    }

    fn stake_gm(&self, hint: &StakeLpTokenHint) -> gmsol_solana_utils::Result<Instruction> {
        let SharedArgs {
            owner,
            position_id,
            global_state,
            lp_mint,
            position,
            position_vault,
            gt_store,
            gt_program,
        } = self.shared_args();
        let token_program_id = anchor_spl::token::ID;
        let market = self.store_program.find_market_address(&lp_mint);
        let controller =
            self.lp_program
                .find_lp_token_controller_address(&global_state, &lp_mint, 0);

        Ok(self
            .lp_program
            .anchor_instruction(args::StakeGm {
                position_id,
                gm_staked_amount: self.amount.get(),
            })
            .anchor_accounts(
                accounts::StakeGm {
                    global_state,
                    controller,
                    lp_mint,
                    position,
                    position_vault,
                    gt_store,
                    gt_program,
                    owner,
                    user_lp_token: self.lp_token_account(&token_program_id),
                    token_map: hint.token_map.0,
                    oracle: self.oracle.0,
                    market,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    event_authority: self.store_program.find_event_authority_address(),
                },
                false,
            )
            .accounts(self.feeds(hint)?)
            .build())
    }

    fn stake_glv(&self, hint: &StakeLpTokenHint) -> gmsol_solana_utils::Result<Instruction> {
        let SharedArgs {
            owner,
            position_id,
            global_state,
            lp_mint,
            position,
            position_vault,
            gt_store,
            gt_program,
        } = self.shared_args();
        let token_program_id = anchor_spl::token_2022::ID;
        let glv = self.store_program.find_glv_address(&lp_mint);
        let market_tokens = hint.glv_market_tokens.as_ref().ok_or_else(|| {
            gmsol_solana_utils::Error::custom("Hint must include the market token list for the GLV")
        })?;
        let glv_accounts = split_to_accounts(
            market_tokens.iter().map(|token| token.0),
            &glv,
            &gt_store,
            &gt_program,
            &token_program_id,
            false,
        )
        .0;

        let controller =
            self.lp_program
                .find_lp_token_controller_address(&global_state, &lp_mint, 0);

        Ok(self
            .lp_program
            .anchor_instruction(args::StakeGlv {
                position_id,
                glv_staked_amount: self.amount.get(),
            })
            .anchor_accounts(
                accounts::StakeGlv {
                    global_state,
                    controller,
                    lp_mint,
                    position,
                    position_vault,
                    gt_store,
                    gt_program,
                    owner,
                    user_lp_token: self.lp_token_account(&token_program_id),
                    token_map: hint.token_map.0,
                    oracle: self.oracle.0,
                    glv,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    event_authority: self.store_program.find_event_authority_address(),
                },
                false,
            )
            .accounts(glv_accounts)
            .accounts(self.feeds(hint)?)
            .build())
    }
}

impl IntoAtomicGroup for StakeLpToken {
    type Hint = StakeLpTokenHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let stake = match self.lp_token_kind {
            LpTokenKind::Gm => self.stake_gm(hint),
            LpTokenKind::Glv => self.stake_glv(hint),
        }?;

        insts.add(stake);

        Ok(insts)
    }
}

/// Hint for [`StakeLpToken`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct StakeLpTokenHint {
    /// Token map.
    #[builder(setter(into))]
    pub token_map: StringPubkey,
    /// Feeds.
    #[builder(setter(into))]
    pub feeds: Vec<SerdeTokenRecord>,
    /// Market tokens (GLV only).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub glv_market_tokens: Option<BTreeSet<StringPubkey>>,
}

impl StakeLpTokenHint {
    /// Create [`TokensWithFeed`].
    pub fn to_tokens_with_feeds(&self) -> gmsol_solana_utils::Result<TokensWithFeed> {
        to_tokens_with_feeds(&self.feeds).map_err(gmsol_solana_utils::Error::custom)
    }
}

impl FromRpcClientWith<StakeLpToken> for StakeLpTokenHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a StakeLpToken,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        let store_program = &builder.store_program;
        let store_address = &store_program.store.0;
        let store = client
            .get_anchor_account::<ZeroCopy<Store>>(store_address, Default::default())
            .await?
            .0;
        let token_map_address = optional_address(&store.token_map)
            .ok_or_else(|| gmsol_solana_utils::Error::custom("token map is not set"))?;

        let (tokens, glv_market_tokens) = match builder.lp_token_kind {
            LpTokenKind::Gm => {
                let market_address = store_program.find_market_address(&builder.lp_token_mint);
                let market = client
                    .get_anchor_account::<ZeroCopy<Market>>(&market_address, Default::default())
                    .await?
                    .0;
                (ordered_tokens(&market.meta.into()), None)
            }
            LpTokenKind::Glv => {
                let glv_address = store_program.find_glv_address(&builder.lp_token_mint);
                let glv = client
                    .get_anchor_account::<ZeroCopy<Glv>>(&glv_address, Default::default())
                    .await?
                    .0;
                let mut collector = glv.tokens_collector(None::<&SwapActionParams>);
                for token in glv.market_tokens() {
                    let market_address = store_program.find_market_address(&token);
                    let market = client
                        .get_anchor_account::<ZeroCopy<Market>>(&market_address, Default::default())
                        .await?
                        .0;
                    collector.insert_token(&market.meta.index_token_mint);
                }
                let market_tokens = glv.market_tokens().map(StringPubkey).collect();
                (collector.unique_tokens(), Some(market_tokens))
            }
        };

        let token_map = client
            .get_anchor_account::<TokenMap>(token_map_address, Default::default())
            .await?;
        let feeds = token_records(&token_map, &tokens)
            .map_err(gmsol_solana_utils::Error::custom)?
            .into_iter()
            .map(SerdeTokenRecord::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)?;

        Ok(Self {
            token_map: (*token_map_address).into(),
            feeds,
            glv_market_tokens,
        })
    }
}

#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Copy)]
pub enum LpTokenKind {
    /// GM.
    Gm,
    /// GLV.
    Glv,
}

struct SharedArgs {
    owner: Pubkey,
    position_id: u64,
    global_state: Pubkey,
    lp_mint: Pubkey,
    position: Pubkey,
    position_vault: Pubkey,
    gt_store: Pubkey,
    gt_program: Pubkey,
}

/// Builder for LP program initialization instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct InitializeLp {
    /// Payer (a.k.a. authority).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Minimum stake value in USD scaled by 1e20.
    pub min_stake_value: u128,
    /// Initial APY for all buckets (1e20-scaled).
    pub initial_apy: u128,
}

impl IntoAtomicGroup for InitializeLp {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;
        let mut insts = AtomicGroup::new(&payer);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::Initialize {
                min_stake_value: self.min_stake_value,
                initial_apy: self.initial_apy,
            })
            .anchor_accounts(
                accounts::Initialize {
                    global_state,
                    authority: payer,
                    system_program: system_program::ID,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token controller creation instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateLpTokenController {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
}

impl IntoAtomicGroup for CreateLpTokenController {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();
        let controller = self
            .lp_program
            .find_lp_token_controller_address(&global_state, &self.lp_token_mint.0);

        let instruction = self
            .lp_program
            .anchor_instruction(args::CreateLpTokenController {
                lp_token_mint: self.lp_token_mint.0,
            })
            .anchor_accounts(
                accounts::CreateLpTokenController {
                    global_state,
                    controller,
                    authority,
                    system_program: system_program::ID,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token controller disable instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct DisableLpTokenController {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
}

impl IntoAtomicGroup for DisableLpTokenController {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();
        let controller = self
            .lp_program
            .find_lp_token_controller_address(&global_state, &self.lp_token_mint.0);

        let instruction = self
            .lp_program
            .anchor_instruction(args::DisableLpTokenController {})
            .anchor_accounts(
                accounts::DisableLpTokenController {
                    global_state,
                    controller,
                    gt_store: self.store_program.store.0,
                    gt_program: *self.store_program.id(),
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token unstaking instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UnstakeLpToken {
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token kind.
    pub lp_token_kind: LpTokenKind,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// LP token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub lp_token_account: Option<StringPubkey>,
    /// Position ID.
    pub position_id: u64,
    /// Unstake amount.
    pub unstake_amount: u64,
}

impl UnstakeLpToken {
    fn lp_token_account(&self, token_program_id: &Pubkey) -> Pubkey {
        self.lp_token_account
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                anchor_spl::associated_token::get_associated_token_address_with_program_id(
                    &self.payer,
                    &self.lp_token_mint,
                    token_program_id,
                )
            })
    }

    fn shared_unstake_args(&self) -> SharedUnstakeArgs {
        let owner = self.payer.0;
        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = self
            .lp_program
            .find_lp_token_controller_address(&global_state, &lp_mint);

        let position =
            self.lp_program
                .find_stake_position_address(&owner, self.position_id, &controller);
        let position_vault = self.lp_program.find_stake_position_vault_address(&position);

        SharedUnstakeArgs {
            owner,
            global_state,
            lp_mint,
            controller,
            position,
            position_vault,
            gt_store: self.store_program.store.0,
            gt_program: *self.store_program.id(),
        }
    }
}

impl IntoAtomicGroup for UnstakeLpToken {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let SharedUnstakeArgs {
            owner,
            global_state,
            lp_mint,
            controller,
            position,
            position_vault,
            gt_store,
            gt_program,
        } = self.shared_unstake_args();

        // Use GT program's find_user_address for gt_user
        let gt_user = crate::pda::find_user_address(&gt_store, &owner, &gt_program).0;
        let event_authority = self.store_program.find_event_authority_address();

        // Token program depends on LP token kind: GM uses token::ID, GLV uses token_2022::ID
        let token_program_id = match self.lp_token_kind {
            LpTokenKind::Gm => anchor_spl::token::ID,
            LpTokenKind::Glv => anchor_spl::token_2022::ID,
        };

        let instruction = self
            .lp_program
            .anchor_instruction(args::UnstakeLp {
                _position_id: self.position_id,
                unstake_amount: self.unstake_amount,
            })
            .anchor_accounts(
                accounts::UnstakeLp {
                    global_state,
                    controller,
                    lp_mint,
                    store: gt_store,
                    gt_program,
                    position,
                    position_vault,
                    owner,
                    gt_user,
                    user_lp_token: self.lp_token_account(&token_program_id),
                    event_authority,
                    token_program: token_program_id,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

struct SharedUnstakeArgs {
    owner: Pubkey,
    global_state: Pubkey,
    lp_mint: Pubkey,
    controller: Pubkey,
    position: Pubkey,
    position_vault: Pubkey,
    gt_store: Pubkey,
    gt_program: Pubkey,
}

/// Builder for transferring LP program authority instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct TransferAuthority {
    /// Current authority.
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// New authority.
    #[builder(setter(into))]
    pub new_authority: StringPubkey,
}

impl IntoAtomicGroup for TransferAuthority {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::TransferAuthority {
                new_authority: self.new_authority.0,
            })
            .anchor_accounts(
                accounts::TransferAuthority {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for accepting LP program authority instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct AcceptAuthority {
    /// Pending authority.
    #[builder(setter(into))]
    pub pending_authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
}

impl IntoAtomicGroup for AcceptAuthority {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let pending_authority = self.pending_authority.0;
        let mut insts = AtomicGroup::new(&pending_authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::AcceptAuthority {})
            .anchor_accounts(
                accounts::AcceptAuthority {
                    global_state,
                    pending_authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for setting pricing staleness configuration instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetPricingStaleness {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Staleness threshold in seconds.
    pub staleness_seconds: u32,
}

impl IntoAtomicGroup for SetPricingStaleness {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::SetPricingStaleness {
                staleness_seconds: self.staleness_seconds,
            })
            .anchor_accounts(
                accounts::SetPricingStaleness {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for setting claim enabled status instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetClaimEnabled {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Whether claiming is enabled.
    pub enabled: bool,
}

impl IntoAtomicGroup for SetClaimEnabled {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::SetClaimEnabled {
                enabled: self.enabled,
            })
            .anchor_accounts(
                accounts::SetClaimEnabled {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for updating APY gradient with sparse entries instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateApyGradientSparse {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Bucket indices to update.
    pub bucket_indices: Vec<u8>,
    /// APY values (1e20-scaled).
    pub apy_values: Vec<u128>,
}

impl IntoAtomicGroup for UpdateApyGradientSparse {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::UpdateApyGradientSparse {
                bucket_indices: self.bucket_indices,
                apy_values: self.apy_values,
            })
            .anchor_accounts(
                accounts::UpdateApyGradientSparse {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for updating APY gradient with range entries instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateApyGradientRange {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Start bucket index.
    pub start_bucket: u8,
    /// End bucket index.
    pub end_bucket: u8,
    /// APY values (1e20-scaled).
    pub apy_values: Vec<u128>,
}

impl IntoAtomicGroup for UpdateApyGradientRange {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::UpdateApyGradientRange {
                start_bucket: self.start_bucket,
                end_bucket: self.end_bucket,
                apy_values: self.apy_values,
            })
            .anchor_accounts(
                accounts::UpdateApyGradientRange {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for updating minimum stake value instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateMinStakeValue {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// New minimum stake value (1e20-scaled).
    pub new_min_stake_value: u128,
}

impl IntoAtomicGroup for UpdateMinStakeValue {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::UpdateMinStakeValue {
                new_min_stake_value: self.new_min_stake_value,
            })
            .anchor_accounts(
                accounts::UpdateMinStakeValue {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token GT reward calculation instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CalculateGtReward {
    /// Owner of the position.
    #[builder(setter(into))]
    pub owner: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// Position ID.
    pub position_id: u64,
}

/// Builder for claiming GT rewards instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct ClaimGtReward {
    /// Owner of the position.
    #[builder(setter(into))]
    pub owner: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// Position ID.
    pub position_id: u64,
}

impl IntoAtomicGroup for CalculateGtReward {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.owner.0;
        let mut insts = AtomicGroup::new(&owner);

        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = self
            .lp_program
            .find_lp_token_controller_address(&global_state, &lp_mint);

        let position =
            self.lp_program
                .find_stake_position_address(&owner, self.position_id, &controller);

        let instruction = self
            .lp_program
            .anchor_instruction(args::CalculateGtReward {})
            .anchor_accounts(
                accounts::CalculateGtReward {
                    global_state,
                    controller,
                    gt_store: self.store_program.store.0,
                    gt_program: *self.store_program.id(),
                    position,
                    owner,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

impl IntoAtomicGroup for ClaimGtReward {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.owner.0;
        let mut insts = AtomicGroup::new(&owner);

        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = self
            .lp_program
            .find_lp_token_controller_address(&global_state, &lp_mint);

        let position =
            self.lp_program
                .find_stake_position_address(&owner, self.position_id, &controller);

        // Use GT program's find_user_address for gt_user
        let gt_user = crate::pda::find_user_address(
            &self.store_program.store.0,
            &owner,
            self.store_program.id(),
        )
        .0;
        let event_authority = self.store_program.find_event_authority_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::ClaimGt {
                _position_id: self.position_id,
            })
            .anchor_accounts(
                accounts::ClaimGt {
                    global_state,
                    controller,
                    store: self.store_program.store.0,
                    gt_program: *self.store_program.id(),
                    position,
                    owner,
                    gt_user,
                    event_authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}
