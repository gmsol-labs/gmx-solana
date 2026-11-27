use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};
use gmsol_store::states::{user::ReferralCodeV2, Seed, Store, UserHeader};
use gmsol_utils::InitSpace;

mod intent_verifier;

declare_id!("Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6");

/// Seed prefix for a permanent EVM proxy account.
#[constant]
pub const PROXY_SEED: &[u8] = b"proxy";
/// Current proxy account layout version.
pub const PROXY_VERSION: u8 = 1;
/// Length of an EVM address in bytes.
pub const EVM_ADDRESS_LEN: usize = 20;
/// Bytes reserved for future proxy account fields.
pub const PROXY_RESERVED_LEN: usize = 248;
const PROXY_PADDING_0_LEN: usize = 6;
const PROXY_PADDING_1_LEN: usize = 4;

/// Intent-kind tag for a consumed Cancel intent.
pub const INTENT_KIND_CANCEL: u8 = 1;

/// Nonce consumption by a successful intent.
#[event]
pub struct IntentConsumed {
    /// Proxy whose nonce was consumed.
    pub proxy: Pubkey,
    /// Nonce consumed by this instruction.
    pub nonce: u64,
    /// Kind of the successful intent.
    pub kind: u8,
}

/// Intent-kind tag for a consumed Withdraw intent.
pub const INTENT_KIND_WITHDRAW: u8 = 2;

/// Transfer completed by a signed Withdraw intent.
#[event]
pub struct Withdrawn {
    /// Proxy that authorized the transfer.
    pub proxy: Pubkey,
    /// Nonce consumed by the transfer.
    pub nonce: u64,
    /// Legacy SPL mint transferred.
    pub mint: Pubkey,
    /// Exact amount transferred in base units.
    pub amount: u64,
    /// Recipient wallet whose canonical ATA received the transfer.
    pub recipient: Pubkey,
}

/// Intent-kind tag for referral-code registration.
pub const INTENT_KIND_SET_REFERRAL_CODE: u8 = 3;
/// Width of the canonical signed referral code.
pub const REFERRAL_CODE_LEN: usize = ReferralCodeV2::LEN;

/// Referral code registered by a successful signed intent.
#[event]
pub struct ReferralCodeSet {
    /// Proxy that owns the registered code.
    pub proxy: Pubkey,
    /// Nonce consumed by registration.
    pub nonce: u64,
    /// Canonical signed code bytes.
    pub code: [u8; REFERRAL_CODE_LEN],
}

/// Permanent account for one EVM address.
#[account(zero_copy)]
pub struct Proxy {
    /// Layout version, set when the proxy is created.
    pub version: u8,
    /// Canonical PDA bump for this EVM address.
    pub bump: u8,
    /// Explicit padding before the EVM address.
    pub padding_0: [u8; PROXY_PADDING_0_LEN],
    /// EVM address permanently associated with this proxy.
    pub evm_address: [u8; EVM_ADDRESS_LEN],
    /// Explicit padding before the nonce.
    pub padding_1: [u8; PROXY_PADDING_1_LEN],
    /// Next expected intent nonce, initially zero.
    pub next_nonce: u64,
    /// Future fields take bytes from this region without reallocating the account.
    pub reserved: [u8; PROXY_RESERVED_LEN],
}

impl InitSpace for Proxy {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Proxy {
    /// Consume the expected nonce and store its checked successor.
    ///
    /// # Errors
    /// Returns `InvalidIntentNonce` for a mismatch or `NonceExhausted` when the
    /// matching nonce is `u64::MAX`. Failure leaves the account unchanged.
    pub fn consume_next_nonce(&mut self, nonce: u64) -> Result<()> {
        require_eq!(nonce, self.next_nonce, ProxyError::InvalidIntentNonce);
        let next = nonce
            .checked_add(1)
            .ok_or_else(|| error!(ProxyError::NonceExhausted))?;
        self.next_nonce = next;
        Ok(())
    }
}

/// Seed prefix for the singleton policy account.
#[constant]
pub const CONFIG_SEED: &[u8] = b"config";
/// Maximum number of allowed full-width EVM chain IDs.
pub const MAX_ALLOWED_CHAINS: usize = 32;

/// Program-wide policy, initialized once by the current upgrade authority.
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Store bound to this deployment for its lifetime.
    pub store: Pubkey,
    /// Immutable cluster genesis hash supplied at initialization.
    pub genesis_hash: [u8; 32],
    /// Current administrator of the intent policy.
    pub admin: Pubkey,
    /// Nominated address that must sign acceptance before taking control.
    pub next_admin: Option<Pubkey>,
    /// Whether every intent is disabled.
    pub paused: bool,
    /// Allowed EVM chain IDs, encoded as full-width big-endian bytes.
    #[max_len(MAX_ALLOWED_CHAINS)]
    pub allowed_chain_ids: Vec<[u8; 32]>,
    /// Canonical PDA bump for this Config.
    pub bump: u8,
}

/// Config initialization event.
#[event]
pub struct ConfigCreated {
    /// Singleton policy account.
    pub config: Pubkey,
    /// Store bound to this deployment for its lifetime.
    pub store: Pubkey,
    /// Immutable cluster genesis hash supplied at initialization.
    pub genesis_hash: [u8; 32],
    /// Current administrator of the intent policy.
    pub admin: Pubkey,
    /// Whether every intent is disabled.
    pub paused: bool,
    /// Allowed EVM chain IDs, encoded as full-width big-endian bytes.
    pub allowed_chain_ids: Vec<[u8; 32]>,
}

/// Allowlist membership-change event.
#[event]
pub struct ConfigChainAllowedChanged {
    /// Singleton policy account.
    pub config: Pubkey,
    /// Full-width big-endian EVM chain ID.
    pub chain_id: [u8; 32],
    /// Resulting allow status for this chain.
    pub allowed: bool,
}

/// Pause-state change event.
#[event]
pub struct ConfigPauseChanged {
    /// Singleton policy account.
    pub config: Pubkey,
    /// Pause state before the change.
    pub previous_paused: bool,
    /// Whether every intent is disabled.
    pub paused: bool,
}

/// Pending-admin proposal or replacement event.
#[event]
pub struct ConfigAdminProposed {
    /// Singleton policy account.
    pub config: Pubkey,
    /// Current administrator of the intent policy.
    pub admin: Pubkey,
    /// Nominee replaced by this proposal, if any.
    pub previous_next_admin: Option<Pubkey>,
    /// Nominated address, required to sign acceptance.
    pub next_admin: Pubkey,
}

/// Admin-handover acceptance event.
#[event]
pub struct ConfigAdminAccepted {
    /// Singleton policy account.
    pub config: Pubkey,
    /// Administrator replaced by this acceptance.
    pub previous_admin: Pubkey,
    /// Current administrator of the intent policy.
    pub admin: Pubkey,
    /// Pending nomination after acceptance; always `None`.
    pub next_admin: Option<Pubkey>,
}

/// EVM proxy instructions.
#[program]
pub mod gmsol_evm_proxy {
    use super::*;

    /// Bind the singleton Config to one Store and a caller-supplied genesis hash.
    /// The current program upgrade authority must sign; the program does not
    /// independently attest the supplied hash against the cluster.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](CreateConfig).*
    ///
    /// # Arguments
    /// - `genesis_hash`: Cluster genesis hash checked by the initializing SDK.
    ///
    /// # Errors
    /// - Returns `NotUpgradeAuthority` for a wrong upgrade authority and
    ///   `InvalidProgramData` for unrelated upgrade metadata. Anchor rejects a
    ///   missing signer, noncanonical Config or invalid Store. The System program
    ///   rejects an existing Config with `AccountAlreadyInUse`.
    ///   Returns `InvalidAdmin` for a zero first admin and `InvalidStore` when the
    ///   Store account is too short to load.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn create_config(ctx: Context<CreateConfig>, genesis_hash: [u8; 32]) -> Result<()> {
        let first_admin = ctx.accounts.first_admin.key();
        require_keys_neq!(first_admin, Pubkey::default(), ProxyError::InvalidAdmin);
        require!(
            ctx.accounts.store.to_account_info().data_len()
                >= 8 + <gmsol_store::states::Store as InitSpace>::INIT_SPACE,
            ProxyError::InvalidStore
        );
        ctx.accounts.store.load()?;
        let config = &mut ctx.accounts.config;
        config.bump = ctx.bumps.config;
        config.store = ctx.accounts.store.key();
        config.genesis_hash = genesis_hash;
        config.admin = first_admin;
        config.next_admin = None;
        config.paused = true;
        config.allowed_chain_ids = Vec::new();
        emit_cpi!(ConfigCreated {
            config: config.key(),
            store: config.store,
            genesis_hash,
            admin: first_admin,
            paused: config.paused,
            allowed_chain_ids: config.allowed_chain_ids.clone(),
        });
        Ok(())
    }

    /// Change whether one full-width EIP-155 chain ID is allowed.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](CurrentConfigAdmin).*
    ///
    /// # Arguments
    /// - `chain_id`: Full-width big-endian EVM chain ID.
    /// - `allowed`: Desired membership.
    ///
    /// # Errors
    /// - Returns `NotConfigAdmin` for a signer other than the current admin.
    ///   Anchor rejects a missing signer or noncanonical Config.
    ///   Returns `NoConfigChange` if the requested status already holds and
    ///   `AllowedChainsFull` if the bounded set cannot accept another chain.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn set_chain_allowed(
        ctx: Context<CurrentConfigAdmin>,
        chain_id: [u8; 32],
        allowed: bool,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        let position = config
            .allowed_chain_ids
            .iter()
            .position(|id| *id == chain_id);
        match (allowed, position) {
            (true, None) => {
                require!(
                    config.allowed_chain_ids.len() < MAX_ALLOWED_CHAINS,
                    ProxyError::AllowedChainsFull
                );
                config.allowed_chain_ids.push(chain_id);
            }
            (false, Some(index)) => {
                config.allowed_chain_ids.remove(index);
            }
            _ => return err!(ProxyError::NoConfigChange),
        }
        emit_cpi!(ConfigChainAllowedChanged {
            config: config.key(),
            chain_id,
            allowed,
        });
        Ok(())
    }

    /// Pause or unpause the intent policy recorded in Config.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](CurrentConfigAdmin).*
    ///
    /// # Arguments
    /// - `paused`: Desired intent pause state.
    ///
    /// # Errors
    /// - Returns `NotConfigAdmin` for a signer other than the current admin.
    ///   Anchor rejects a missing signer or noncanonical Config.
    ///   Returns `NoConfigChange` when the requested pause state already holds.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn set_paused(ctx: Context<CurrentConfigAdmin>, paused: bool) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(config.paused != paused, ProxyError::NoConfigChange);
        let previous_paused = config.paused;
        config.paused = paused;
        emit_cpi!(ConfigPauseChanged {
            config: config.key(),
            previous_paused,
            paused,
        });
        Ok(())
    }

    /// Nominate or replace the next admin without transferring current control.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](TransferAdmin).*
    ///
    /// # Arguments
    /// None.
    ///
    /// # Errors
    /// - Returns `NotConfigAdmin` for a signer other than the current admin.
    ///   Anchor rejects a missing signer or noncanonical Config.
    ///   Returns `InvalidAdmin` for the zero address or `NoConfigChange` when
    ///   nominating the current or already pending admin.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn transfer_admin(ctx: Context<TransferAdmin>) -> Result<()> {
        let next_admin = ctx.accounts.next_admin.key();
        let config = &mut ctx.accounts.config;
        require_keys_neq!(next_admin, Pubkey::default(), ProxyError::InvalidAdmin);
        require_keys_neq!(next_admin, config.admin, ProxyError::NoConfigChange);
        require!(
            config.next_admin != Some(next_admin),
            ProxyError::NoConfigChange
        );
        let previous_next_admin = config.next_admin;
        config.next_admin = Some(next_admin);
        emit_cpi!(ConfigAdminProposed {
            config: config.key(),
            admin: config.admin,
            previous_next_admin,
            next_admin,
        });
        Ok(())
    }

    /// Complete a nominated admin handover with the nominee's signature.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](AcceptAdmin).*
    ///
    /// # Arguments
    /// None.
    ///
    /// # Errors
    /// - Anchor rejects a missing signer or noncanonical Config. Returns
    ///   `NotPendingAdmin` when the signer is not the nominated admin.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(
            config.next_admin == Some(ctx.accounts.next_admin.key()),
            ProxyError::NotPendingAdmin
        );
        let previous_admin = config.admin;
        config.admin = ctx.accounts.next_admin.key();
        config.next_admin = None;
        emit_cpi!(ConfigAdminAccepted {
            config: config.key(),
            previous_admin,
            admin: config.admin,
            next_admin: config.next_admin,
        });
        Ok(())
    }

    /// Invoke the program scaffold without creating or changing a proxy.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](Initialize).*
    ///
    /// # Arguments
    /// None.
    ///
    /// # Errors
    /// - Anchor account validation fails for invalid accounts.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }

    /// Create a permanent proxy, or validate an existing proxy without changing it.
    /// The payer gains no authority over the proxy.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](CreateProxyIdempotent).*
    ///
    /// # Arguments
    /// - `evm_address`: Raw EVM address identifying the proxy.
    ///
    /// # Errors
    /// - [`ProxyError::InvalidProxy`] if an existing proxy has a different version,
    ///   bump, or EVM address.
    /// - Anchor seed/account validation or System account creation fails.
    pub fn create_proxy_idempotent(
        ctx: Context<CreateProxyIdempotent>,
        evm_address: [u8; EVM_ADDRESS_LEN],
    ) -> Result<()> {
        match ctx.accounts.proxy.load_init() {
            Ok(mut proxy) => {
                proxy.version = PROXY_VERSION;
                proxy.bump = ctx.bumps.proxy;
                proxy.evm_address = evm_address;
                proxy.next_nonce = 0;
            }
            Err(Error::AnchorError(err))
                if err.error_code_number == ErrorCode::AccountDiscriminatorAlreadySet as u32 => {}
            Err(err) => return Err(err),
        }
        ctx.accounts.proxy.exit(&crate::ID)?;
        let proxy = ctx.accounts.proxy.load()?;
        require_eq!(proxy.version, PROXY_VERSION, ProxyError::InvalidProxy);
        require_eq!(proxy.bump, ctx.bumps.proxy, ProxyError::InvalidProxy);
        require!(proxy.evm_address == evm_address, ProxyError::InvalidProxy);
        Ok(())
    }

    /// Consume the next signed Cancel nonce with checked advancement,
    /// without changing the proxy's business state.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](Cancel).*
    ///
    /// # Arguments
    /// - `chain_id`: Signed full-width EVM chain ID.
    /// - `nonce`: Signed next nonce.
    /// - `deadline`: Signed execution deadline.
    /// - `signature`: Compact ECDSA signature over the Cancel digest.
    /// - `recovery_id`: Canonical recovery ID, zero or one.
    ///
    /// # Errors
    /// - Anchor account validation, account borrowing or Clock access fails.
    /// - [`ProxyError::InvalidProxy`] for an unsupported stored layout version.
    /// - [`ProxyError::Paused`] while Config is paused.
    /// - [`ProxyError::ChainNotAllowed`] when the chain ID is not allowed.
    /// - [`ProxyError::InvalidDeadline`] for a negative Clock timestamp or a
    ///   deadline outside the inclusive window from now through now plus 3600 seconds.
    /// - [`ProxyError::InvalidSignature`] for an invalid recovery ID, zero or
    ///   high `s`, failed recovery or a recovered address differing from the EVM owner.
    /// - [`ProxyError::InvalidIntentNonce`] when a verified intent's nonce is not next.
    /// - [`ProxyError::NonceExhausted`] when the verified matching nonce is u64::MAX.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn cancel(
        ctx: Context<Cancel>,
        chain_id: [u8; 32],
        nonce: u64,
        deadline: u64,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> Result<()> {
        let proxy_key = ctx.accounts.proxy.key();
        let evm_address;
        {
            let proxy = ctx.accounts.proxy.load()?;
            require_eq!(proxy.version, PROXY_VERSION, ProxyError::InvalidProxy);
            evm_address = proxy.evm_address;
        }

        let struct_hash = gmsol_evm_proxy_intent::cancel_struct_hash(&proxy_key, nonce, deadline);
        intent_verifier::verify(
            &ctx.accounts.config,
            evm_address,
            chain_id,
            deadline,
            struct_hash,
            signature,
            recovery_id,
        )?;

        ctx.accounts.proxy.load_mut()?.consume_next_nonce(nonce)?;
        emit_cpi!(IntentConsumed {
            proxy: proxy_key,
            nonce,
            kind: INTENT_KIND_CANCEL,
        });
        Ok(())
    }
    /// Transfer a signed amount of a legacy SPL mint from the proxy ATA to the
    /// existing ATA of the signed recipient wallet. Any Solana payer may submit
    /// the EVM owner's signature; the payer does not become the token recipient.
    /// The signature binds the mint, recipient wallet, amount, decimals, nonce,
    /// deadline, chain, and deployment domain. Both token accounts must exist,
    /// use the legacy Token program, and be unfrozen.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](Withdraw).*
    ///
    /// # Arguments
    /// - `chain_id`: Signed EVM chain ID.
    /// - `amount`: Signed transfer amount in base units.
    /// - `decimals`: Signed mint decimals.
    /// - `nonce`: Signed next counter value.
    /// - `deadline`: Signed execution deadline.
    /// - `signature`: Compact ECDSA signature.
    /// - `recovery_id`: Canonical recovery ID, zero or one.
    ///
    /// # Errors
    /// - Returns `ZeroAmount` for zero, `SelfRecipient` when the recipient wallet
    ///   is the proxy, and `InvalidDecimals` for a mint decimal mismatch. Account
    ///   constraints reject noncanonical or missing ATAs, wrong mint or Token
    ///   program. The checked token transfer rejects frozen token accounts.
    /// - Returns `InvalidProxy` for an unsupported proxy version and
    ///   `InvalidIntentNonce` for any non-next nonce. Verification returns `Paused`,
    ///   `ChainNotAllowed`, `InvalidDeadline` or `InvalidSignature` for policy,
    ///   deadline or EVM-signature rejection. Loading, borrowing and Clock errors
    ///   propagate. NonceExhausted rejects a verified matching u64::MAX before transfer.
    ///   A failed transfer leaves the nonce unconsumed and token balances unchanged.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn withdraw(
        ctx: Context<Withdraw>,
        chain_id: [u8; 32],
        amount: u64,
        decimals: u8,
        nonce: u64,
        deadline: u64,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> Result<()> {
        require!(amount > 0, ProxyError::ZeroAmount);
        let proxy_key = ctx.accounts.proxy.key();
        let recipient = ctx.accounts.recipient.key();
        require_keys_neq!(proxy_key, recipient, ProxyError::SelfRecipient);
        require_eq!(
            decimals,
            ctx.accounts.mint.decimals,
            ProxyError::InvalidDecimals
        );
        let (evm_address, proxy_bump);
        {
            let proxy = ctx.accounts.proxy.load()?;
            require_eq!(proxy.version, PROXY_VERSION, ProxyError::InvalidProxy);
            evm_address = proxy.evm_address;
            proxy_bump = proxy.bump;
        }
        let mint = ctx.accounts.mint.key();
        let struct_hash =
            gmsol_evm_proxy_intent::withdraw_struct_hash(&gmsol_evm_proxy_intent::WithdrawFields {
                proxy: proxy_key,
                mint,
                amount,
                decimals,
                recipient,
                nonce,
                deadline,
            });
        intent_verifier::verify(
            &ctx.accounts.config,
            evm_address,
            chain_id,
            deadline,
            struct_hash,
            signature,
            recovery_id,
        )?;
        ctx.accounts.proxy.load_mut()?.consume_next_nonce(nonce)?;
        let bump = [proxy_bump];
        let signer_seeds: &[&[u8]] = &[PROXY_SEED, evm_address.as_ref(), &bump];
        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.source.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: ctx.accounts.proxy.to_account_info(),
                },
            )
            .with_signer(&[signer_seeds]),
            amount,
            decimals,
        )?;
        emit_cpi!(IntentConsumed {
            proxy: proxy_key,
            nonce,
            kind: INTENT_KIND_WITHDRAW,
        });
        emit_cpi!(Withdrawn {
            proxy: proxy_key,
            nonce,
            mint,
            amount,
            recipient
        });
        Ok(())
    }
    /// Register the EVM owner's canonical code through Store.
    /// The submitter funds only missing targets' rent deficits. The proxy signs
    /// the Store CPIs as owner without paying rent from its data-bearing account.
    /// Create the Proxy first, or earlier in the same transaction. Existing target
    /// balances reduce the submitter's rent cost; excess lamports stay in the targets.
    /// Proxy, User, referral code and submitter must be writable even when prefunded.
    ///
    /// A failed transaction rolls back persistent account, rent and nonce changes,
    /// but its actual fee payer still pays transaction fees. Failed logs may remain
    /// visible; apply events only after confirming success and program attribution.
    ///
    /// # Accounts
    /// *[See the documentation for the accounts.](SetReferralCode).*
    ///
    /// # Arguments
    /// - `chain_id`: signed EVM chain ID.
    /// - `code`: canonical signed referral-code bytes.
    /// - `nonce`: signed next counter value.
    /// - `deadline`: signed execution deadline.
    /// - `signature`: compact ECDSA signature.
    /// - `recovery_id`: canonical recovery ID, zero or one.
    ///
    /// # Errors
    /// - Anchor account validation rejects Store, User or code accounts that do
    ///   not match Config and the signed code bytes.
    /// - [`ProxyError::InvalidReferralCode`] for a zero code.
    /// - [`ProxyError::ReferralCodeTaken`] for an occupied code target.
    /// - [`ProxyError::InvalidReferralUser`] for invalid User ownership or layout.
    /// - [`ProxyError::InvalidConfigStore`] for a Store other than Config's Store.
    /// - [`ProxyError::InvalidProxy`] for an unsupported proxy version.
    /// - [`ProxyError::InvalidIntentNonce`] when the signed nonce is not next.
    /// - [`ProxyError::Paused`], [`ProxyError::ChainNotAllowed`],
    ///   [`ProxyError::InvalidDeadline`] or [`ProxyError::InvalidSignature`]
    ///   for policy, deadline or EVM-signature rejection.
    /// - Account loading/borrowing, Clock, rent, funding and Store CPI errors
    ///   propagate. Insufficient sponsor funds or an already-coded User fail
    ///   atomically, excluding transaction fees.
    /// - NonceExhausted rejects a verified matching u64::MAX before funding or Store CPIs.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn set_referral_code(
        ctx: Context<SetReferralCode>,
        chain_id: [u8; 32],
        code: [u8; REFERRAL_CODE_LEN],
        nonce: u64,
        deadline: u64,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> Result<()> {
        require!(
            code != [0; REFERRAL_CODE_LEN],
            ProxyError::InvalidReferralCode
        );
        let proxy_key = ctx.accounts.proxy.key();
        let (evm_address, proxy_bump);
        {
            let proxy = ctx.accounts.proxy.load()?;
            require_eq!(proxy.version, PROXY_VERSION, ProxyError::InvalidProxy);
            evm_address = proxy.evm_address;
            proxy_bump = proxy.bump;
        }
        intent_verifier::verify(
            &ctx.accounts.config,
            evm_address,
            chain_id,
            deadline,
            gmsol_evm_proxy_intent::set_referral_code_struct_hash(
                &proxy_key,
                &ReferralCodeV2::encode(&code, true),
                nonce,
                deadline,
            ),
            signature,
            recovery_id,
        )?;

        ctx.accounts.proxy.load_mut()?.consume_next_nonce(nonce)?;
        let user = ctx.accounts.user.to_account_info();
        let referral_code = ctx.accounts.referral_code.to_account_info();
        require!(
            referral_code.owner == &system_program::ID && referral_code.data_is_empty(),
            ProxyError::ReferralCodeTaken
        );
        let missing_user = user.owner == &system_program::ID && user.data_is_empty();
        if !missing_user {
            require_keys_eq!(
                *user.owner,
                gmsol_store::ID,
                ProxyError::InvalidReferralUser
            );
            require!(
                user.data_len() >= 8 + UserHeader::space(0)
                    && user
                        .try_borrow_data()?
                        .starts_with(UserHeader::DISCRIMINATOR),
                ProxyError::InvalidReferralUser
            );
        }
        let rent = Rent::get()?;
        if missing_user {
            ctx.accounts
                .fund_rent_deficit(&user, rent.minimum_balance(8 + UserHeader::space(0)))?;
        }
        ctx.accounts.fund_rent_deficit(
            &referral_code,
            rent.minimum_balance(8 + <ReferralCodeV2 as InitSpace>::INIT_SPACE),
        )?;

        let bump = [proxy_bump];
        let seeds: &[&[u8]] = &[PROXY_SEED, evm_address.as_ref(), &bump];
        let signer = &[seeds];
        gmsol_store::cpi::prepare_user(CpiContext::new_with_signer(
            ctx.accounts.store_program.to_account_info(),
            gmsol_store::cpi::accounts::PrepareUser {
                owner: ctx.accounts.proxy.to_account_info(),
                store: ctx.accounts.store.to_account_info(),
                user: user.clone(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
            signer,
        ))?;
        gmsol_store::cpi::initialize_referral_code(
            CpiContext::new_with_signer(
                ctx.accounts.store_program.to_account_info(),
                gmsol_store::cpi::accounts::InitializeReferralCode {
                    owner: ctx.accounts.proxy.to_account_info(),
                    store: ctx.accounts.store.to_account_info(),
                    referral_code,
                    user,
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                signer,
            ),
            code,
        )?;
        emit_cpi!(IntentConsumed {
            proxy: proxy_key,
            nonce,
            kind: INTENT_KIND_SET_REFERRAL_CODE
        });
        emit_cpi!(ReferralCodeSet {
            proxy: proxy_key,
            nonce,
            code
        });
        Ok(())
    }
}

impl<'info> SetReferralCode<'info> {
    /// Fund only the target account's missing rent through an ordinary transfer.
    fn fund_rent_deficit(&self, target: &AccountInfo<'info>, minimum: u64) -> Result<()> {
        let deficit = minimum.saturating_sub(target.lamports());
        if deficit != 0 {
            system_program::transfer(
                CpiContext::new(
                    self.system_program.to_account_info(),
                    system_program::Transfer {
                        from: self.submitter.to_account_info(),
                        to: target.clone(),
                    },
                ),
                deficit,
            )?;
        }
        Ok(())
    }
}

/// Accounts for the scaffold initialization instruction.
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// Caller signing the scaffold invocation.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// System program.
    pub system_program: Program<'info, System>,
}

/// Accounts for one-time Config initialization.
#[event_cpi]
#[derive(Accounts)]
pub struct CreateConfig<'info> {
    /// Signer paying initialization rent.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// Current program upgrade authority signing initialization.
    pub upgrade_authority: Signer<'info>,
    /// Singleton policy account.
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub config: Account<'info, Config>,
    /// This EVM proxy deployment.
    #[account(constraint = proxy_program.programdata_address()? == Some(program_data.key()) @ ProxyError::InvalidProgramData)]
    pub proxy_program: Program<'info, program::GmsolEvmProxy>,
    /// Upgradeable-loader metadata for this deployment.
    #[account(constraint = program_data.upgrade_authority_address == Some(upgrade_authority.key()) @ ProxyError::NotUpgradeAuthority)]
    pub program_data: Account<'info, ProgramData>,
    /// Store bound to this deployment for its lifetime.
    pub store: AccountLoader<'info, gmsol_store::states::Store>,
    /// Initial administrator recorded by Config.
    /// CHECK: only this address is recorded as the first admin.
    pub first_admin: UncheckedAccount<'info>,
    /// System program used to create Config.
    pub system_program: Program<'info, System>,
}

/// Accounts for current-admin policy changes.
#[event_cpi]
#[derive(Accounts)]
pub struct CurrentConfigAdmin<'info> {
    /// Current administrator of the intent policy.
    pub admin: Signer<'info>,
    /// Singleton policy account.
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ProxyError::NotConfigAdmin)]
    pub config: Account<'info, Config>,
}

/// Accounts for accepting a nominated admin handover.
#[event_cpi]
#[derive(Accounts)]
pub struct AcceptAdmin<'info> {
    /// Nominated address, required to sign acceptance.
    pub next_admin: Signer<'info>,
    /// Singleton policy account.
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

/// Accounts for proposing a two-step admin handover.
#[event_cpi]
#[derive(Accounts)]
pub struct TransferAdmin<'info> {
    /// Current admin authorizing the proposal.
    pub admin: Signer<'info>,
    /// Singleton policy account.
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin @ ProxyError::NotConfigAdmin)]
    pub config: Account<'info, Config>,
    /// Nominated admin address.
    /// CHECK: only the address is used; it gains authority only by accepting.
    pub next_admin: UncheckedAccount<'info>,
}

/// Accounts for permissionless, idempotent proxy creation.
#[derive(Accounts)]
#[instruction(evm_address: [u8; EVM_ADDRESS_LEN])]
pub struct CreateProxyIdempotent<'info> {
    /// Pays rent but has no authority over the proxy.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// Permanent PDA for the supplied EVM address.
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + Proxy::INIT_SPACE,
        seeds = [PROXY_SEED, evm_address.as_ref()],
        bump,
    )]
    pub proxy: AccountLoader<'info, Proxy>,
    /// System program used to allocate a new proxy.
    pub system_program: Program<'info, System>,
}

/// Accounts for consuming a signed Cancel intent.
#[event_cpi]
#[derive(Accounts)]
pub struct Cancel<'info> {
    /// Signer submitting the intent; no EVM owner authority is required.
    pub submitter: Signer<'info>,
    /// Singleton policy and immutable domain fields.
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// Permanent proxy identified by its own stored address and bump.
    #[account(mut, seeds = [PROXY_SEED, proxy.load()?.evm_address.as_ref()], bump = proxy.load()?.bump)]
    pub proxy: AccountLoader<'info, Proxy>,
}

/// Accounts for transferring a signed amount between existing legacy ATAs.
#[event_cpi]
#[derive(Accounts)]
pub struct Withdraw<'info> {
    /// Transaction signer; the EVM signature supplies transfer authority.
    pub submitter: Signer<'info>,
    /// Singleton policy and immutable signing domain.
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// Proxy whose stored identity determines the canonical PDA.
    #[account(mut, seeds = [PROXY_SEED, proxy.load()?.evm_address.as_ref()], bump = proxy.load()?.bump)]
    pub proxy: AccountLoader<'info, Proxy>,
    /// Initialized legacy SPL mint.
    pub mint: Account<'info, Mint>,
    /// Recipient wallet bound by the signed intent.
    /// CHECK: the address is signed and the destination ATA is constrained below.
    pub recipient: UncheckedAccount<'info>,
    /// Existing canonical proxy ATA for the legacy SPL Token program and mint.
    #[account(mut, associated_token::mint = mint, associated_token::authority = proxy, associated_token::token_program = token_program)]
    pub source: Account<'info, TokenAccount>,
    /// Existing canonical recipient ATA for the legacy SPL Token program and mint.
    #[account(mut, associated_token::mint = mint, associated_token::authority = recipient, associated_token::token_program = token_program)]
    pub destination: Account<'info, TokenAccount>,
    /// Legacy SPL Token program.
    pub token_program: Program<'info, Token>,
}

/// Accounts for sponsored referral-code registration.
#[event_cpi]
#[derive(Accounts)]
#[instruction(chain_id: [u8; 32], code: [u8; REFERRAL_CODE_LEN])]
pub struct SetReferralCode<'info> {
    /// Sponsors target rent; may differ from the transaction fee payer.
    #[account(mut)]
    pub submitter: Signer<'info>,
    /// Singleton policy and immutable Store binding.
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = store @ ProxyError::InvalidConfigStore)]
    pub config: Account<'info, Config>,
    /// Both Store instructions require a writable owner signer.
    #[account(mut, seeds = [PROXY_SEED, proxy.load()?.evm_address.as_ref()], bump = proxy.load()?.bump)]
    pub proxy: AccountLoader<'info, Proxy>,
    /// Store permanently bound by Config.
    pub store: AccountLoader<'info, Store>,
    /// User account owned by this Proxy in the bound Store.
    /// CHECK: canonical Store PDA; ownership/layout checked before funding; Store validates initialized fields and bump.
    #[account(mut, seeds = [UserHeader::SEED, store.key().as_ref(), proxy.key().as_ref()],
        bump, seeds::program = store_program.key())]
    pub user: UncheckedAccount<'info>,
    /// Referral code account registered to this Proxy.
    /// CHECK: canonical code PDA, required to be empty and System-owned before funding.
    #[account(mut, seeds = [ReferralCodeV2::SEED, store.key().as_ref(), &code],
        bump, seeds::program = store_program.key())]
    pub referral_code: UncheckedAccount<'info>,
    /// Store program used for User preparation and code registration.
    pub store_program: Program<'info, gmsol_store::program::GmsolStore>,
    /// System program used to fund target rent deficits.
    pub system_program: Program<'info, System>,
}

/// EVM proxy account errors.
#[error_code]
pub enum ProxyError {
    /// Existing proxy identity or layout metadata does not match its PDA.
    #[msg("Proxy account data does not match its EVM address")]
    InvalidProxy,
    /// Store data cannot hold the declared Store layout.
    #[msg("Store account data is too short")]
    InvalidStore,
    /// Admin address is zero.
    #[msg("Admin address cannot be zero")]
    InvalidAdmin,
    /// The requested policy or handover change is already in effect.
    #[msg("Config change has no effect")]
    NoConfigChange,
    /// The bounded allowed-chain set is full.
    #[msg("Allowed EVM chain set is full")]
    AllowedChainsFull,
    /// The signer is not the nominated admin.
    #[msg("Signer is not the pending admin")]
    NotPendingAdmin,
    /// ProgramData does not belong to this proxy program.
    #[msg("ProgramData does not belong to this proxy program")]
    InvalidProgramData,
    /// Signer is not the current program upgrade authority.
    #[msg("Signer is not the program upgrade authority")]
    NotUpgradeAuthority,
    /// Signer is not the current Config admin.
    #[msg("Signer is not the current Config admin")]
    NotConfigAdmin,
    /// Signed nonce is not the next accepted counter value.
    #[msg("Intent nonce is not the proxy's next nonce")]
    InvalidIntentNonce,
    /// Intent verification is paused by Config.
    #[msg("Intent verification is paused")]
    Paused,
    /// Signed EVM chain is absent from the allowlist.
    #[msg("EVM chain is not allowed")]
    ChainNotAllowed,
    /// Deadline lies outside the current execution window.
    #[msg("Intent deadline is outside the execution window")]
    InvalidDeadline,
    /// Signature or recovered EVM owner is invalid.
    #[msg("Invalid EVM signature")]
    InvalidSignature,
    /// The matching next nonce has no representable successor.
    #[msg("Proxy nonce is exhausted")]
    NonceExhausted,
    /// Signed transfer amount is zero.
    #[msg("Withdraw amount must be positive")]
    ZeroAmount,
    /// Signed recipient is the source proxy itself.
    #[msg("Proxy cannot withdraw to itself")]
    SelfRecipient,
    /// Signed decimal precision differs from the mint.
    #[msg("Signed decimals differ from the mint")]
    InvalidDecimals,
    /// Signed referral-code input is zero.
    #[msg("Referral code cannot be zero")]
    InvalidReferralCode,
    /// Store User has invalid ownership or layout.
    #[msg("Referral User has invalid ownership or layout")]
    InvalidReferralUser,
    /// Canonical referral-code target is already occupied.
    #[msg("Referral code is already taken")]
    ReferralCodeTaken,
    /// Supplied Store differs from Config's immutable Store.
    #[msg("Store differs from Config's bound Store")]
    InvalidConfigStore,
}

#[cfg(not(feature = "no-entrypoint"))]
gmsol_utils::security_txt!("GMX-Solana EVM Proxy Program");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_account_binary_layout() {
        const EXPECTED_PROXY_ACCOUNT_SIZE: usize = 288;
        const EXPECTED_EVM_ADDRESS_OFFSET: usize = 8;
        const EXPECTED_NEXT_NONCE_OFFSET: usize = 32;
        assert_eq!(std::mem::size_of::<Proxy>(), EXPECTED_PROXY_ACCOUNT_SIZE);
        assert_eq!(Proxy::INIT_SPACE, EXPECTED_PROXY_ACCOUNT_SIZE);
        assert_eq!(std::mem::offset_of!(Proxy, reserved), 40);
        assert_eq!(
            std::mem::offset_of!(Proxy, evm_address),
            EXPECTED_EVM_ADDRESS_OFFSET
        );
        assert_eq!(
            std::mem::offset_of!(Proxy, next_nonce),
            EXPECTED_NEXT_NONCE_OFFSET
        );
    }

    fn assert_nonce_error(error: Error, expected: ProxyError) {
        let Error::AnchorError(error) = error else {
            panic!("expected nonce error");
        };
        assert_eq!(error.error_code_number, u32::from(expected));
    }

    fn proxy_with_nonce(nonce: u64) -> Proxy {
        let mut proxy: Proxy = bytemuck::Zeroable::zeroed();
        proxy.next_nonce = nonce;
        proxy.version = PROXY_VERSION;
        proxy.evm_address = [0x42; EVM_ADDRESS_LEN];
        proxy.reserved.fill(0xa5);
        proxy
    }

    #[test]
    fn next_nonce_starts_at_zero_and_advances_once() {
        let mut proxy: Proxy = bytemuck::Zeroable::zeroed();
        assert_eq!(proxy.next_nonce, 0);
        proxy.consume_next_nonce(0).unwrap();
        assert_eq!(proxy.next_nonce, 1);
        proxy.consume_next_nonce(1).unwrap();
        assert_eq!(proxy.next_nonce, 2);
    }

    #[test]
    fn adjacent_nonces_and_replay_leave_all_state_unchanged() {
        const CURRENT_NONCE: u64 = 7;
        let mut proxy = proxy_with_nonce(CURRENT_NONCE);
        let before = proxy;
        for wrong in [CURRENT_NONCE - 1, CURRENT_NONCE + 1] {
            assert_nonce_error(
                proxy.consume_next_nonce(wrong).unwrap_err(),
                ProxyError::InvalidIntentNonce,
            );
            assert_eq!(bytemuck::bytes_of(&proxy), bytemuck::bytes_of(&before));
        }
        proxy.consume_next_nonce(CURRENT_NONCE).unwrap();
        assert_eq!(proxy.next_nonce, CURRENT_NONCE + 1);
        let before_replay = proxy;
        assert_nonce_error(
            proxy.consume_next_nonce(CURRENT_NONCE).unwrap_err(),
            ProxyError::InvalidIntentNonce,
        );
        assert_eq!(
            bytemuck::bytes_of(&proxy),
            bytemuck::bytes_of(&before_replay)
        );
        proxy.consume_next_nonce(CURRENT_NONCE + 1).unwrap();
        let mut expected = before;
        expected.next_nonce = CURRENT_NONCE + 2;
        assert_eq!(bytemuck::bytes_of(&proxy), bytemuck::bytes_of(&expected));
    }

    #[test]
    fn nonce_exhaustion_never_wraps_or_changes_state() {
        let mut proxy = proxy_with_nonce(u64::MAX - 1);
        proxy.consume_next_nonce(u64::MAX - 1).unwrap();
        assert_eq!(proxy.next_nonce, u64::MAX);
        let exhausted = proxy;
        assert_nonce_error(
            proxy.consume_next_nonce(u64::MAX).unwrap_err(),
            ProxyError::NonceExhausted,
        );
        assert_eq!(bytemuck::bytes_of(&proxy), bytemuck::bytes_of(&exhausted));
        for wrong in [0, u64::MAX - 1] {
            assert_nonce_error(
                proxy.consume_next_nonce(wrong).unwrap_err(),
                ProxyError::InvalidIntentNonce,
            );
            assert_eq!(bytemuck::bytes_of(&proxy), bytemuck::bytes_of(&exhausted));
        }
    }
}
