use anchor_lang::prelude::*;
use gmsol_programs::gmsol_store::accounts::Store;
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

/// Kind of a consumed intent.
#[derive(AnchorSerialize, AnchorDeserialize)]
pub enum IntentKind {
    /// Cancel.
    Cancel,
}

/// Nonce consumption by a successful intent.
#[event]
pub struct IntentConsumed {
    /// Proxy whose nonce was consumed.
    pub proxy: Pubkey,
    /// Nonce consumed by this instruction.
    pub nonce: u64,
    /// Kind of the successful intent.
    pub kind: IntentKind,
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

/// Load a proxy with the current layout version.
///
/// # Errors
/// Returns `InvalidProxy` for another layout version.
fn load_proxy<'a>(proxy: &'a AccountLoader<Proxy>) -> Result<std::cell::Ref<'a, Proxy>> {
    let loaded = proxy.load()?;
    require_eq!(loaded.version, PROXY_VERSION, ProxyError::InvalidProxy);
    Ok(loaded)
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

/// Pending-admin withdrawal event.
#[event]
pub struct ConfigAdminProposalCancelled {
    /// Singleton policy account.
    pub config: Pubkey,
    /// Current administrator of the intent policy.
    pub admin: Pubkey,
    /// Nominee cleared by this withdrawal.
    pub previous_next_admin: Pubkey,
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
    ///   Returns `InvalidAdmin` for a zero first admin.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn create_config(ctx: Context<CreateConfig>, genesis_hash: [u8; 32]) -> Result<()> {
        let first_admin = ctx.accounts.first_admin.key();
        require_keys_neq!(first_admin, Pubkey::default(), ProxyError::InvalidAdmin);
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
    /// Nominating the current admin withdraws the pending nomination.
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
    ///   nominating the already pending admin, or the current admin with no
    ///   pending nomination.
    /// - CPI event emission fails if its supplied accounts are invalid.
    pub fn transfer_admin(ctx: Context<TransferAdmin>) -> Result<()> {
        let next_admin = ctx.accounts.next_admin.key();
        let config = &mut ctx.accounts.config;
        require_keys_neq!(next_admin, Pubkey::default(), ProxyError::InvalidAdmin);
        if next_admin == config.admin {
            let previous_next_admin = config
                .next_admin
                .take()
                .ok_or_else(|| error!(ProxyError::NoConfigChange))?;
            emit_cpi!(ConfigAdminProposalCancelled {
                config: config.key(),
                admin: config.admin,
                previous_next_admin,
            });
            return Ok(());
        }
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
        let proxy = load_proxy(&ctx.accounts.proxy)?;
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
        let intent = gmsol_evm_proxy_intent::Cancel {
            proxy_account: ctx.accounts.proxy.key(),
            nonce,
            deadline,
        };
        intent_verifier::consume(
            &ctx.accounts.config,
            &ctx.accounts.proxy,
            chain_id,
            &intent,
            signature,
            recovery_id,
        )?;
        emit_cpi!(IntentConsumed {
            proxy: intent.proxy_account,
            nonce: intent.nonce,
            kind: IntentKind::Cancel,
        });
        Ok(())
    }
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
    pub store: AccountLoader<'info, Store>,
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
    /// Nominated admin address, or the current admin to withdraw the nomination.
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

/// EVM proxy account errors.
#[error_code]
pub enum ProxyError {
    /// Existing proxy identity or layout metadata does not match its PDA.
    #[msg("Proxy account data does not match its EVM address")]
    InvalidProxy,
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
        const EXPECTED_RESERVED_OFFSET: usize = 40;
        assert_eq!(std::mem::size_of::<Proxy>(), EXPECTED_PROXY_ACCOUNT_SIZE);
        assert_eq!(Proxy::INIT_SPACE, EXPECTED_PROXY_ACCOUNT_SIZE);
        assert_eq!(
            std::mem::offset_of!(Proxy, evm_address),
            EXPECTED_EVM_ADDRESS_OFFSET
        );
        assert_eq!(
            std::mem::offset_of!(Proxy, next_nonce),
            EXPECTED_NEXT_NONCE_OFFSET
        );
        assert_eq!(
            std::mem::offset_of!(Proxy, reserved),
            EXPECTED_RESERVED_OFFSET
        );
    }

    fn proxy_with_nonce(nonce: u64) -> Proxy {
        let mut proxy: Proxy = bytemuck::Zeroable::zeroed();
        proxy.next_nonce = nonce;
        proxy.version = PROXY_VERSION;
        proxy.evm_address = [0x42; EVM_ADDRESS_LEN];
        proxy.reserved.fill(0xa5);
        proxy
    }

    fn assert_rejected(proxy: &mut Proxy, nonce: u64, expected: ProxyError) {
        let before = *proxy;
        let Error::AnchorError(error) = proxy.consume_next_nonce(nonce).unwrap_err() else {
            panic!("expected nonce error");
        };
        assert_eq!(error.error_code_number, u32::from(expected));
        assert_eq!(bytemuck::bytes_of(proxy), bytemuck::bytes_of(&before));
    }

    #[test]
    fn accepts_only_the_next_nonce() {
        const NEXT_NONCE: u64 = 7;
        let mut proxy = proxy_with_nonce(NEXT_NONCE);
        for nonce in [NEXT_NONCE - 1, NEXT_NONCE + 1] {
            assert_rejected(&mut proxy, nonce, ProxyError::InvalidIntentNonce);
        }
        let mut expected = proxy;
        proxy.consume_next_nonce(NEXT_NONCE).unwrap();
        expected.next_nonce = NEXT_NONCE + 1;
        assert_eq!(bytemuck::bytes_of(&proxy), bytemuck::bytes_of(&expected));
        assert_rejected(&mut proxy, NEXT_NONCE, ProxyError::InvalidIntentNonce);
    }

    #[test]
    fn stops_at_the_maximum_nonce() {
        let mut proxy = proxy_with_nonce(u64::MAX - 1);
        proxy.consume_next_nonce(u64::MAX - 1).unwrap();
        assert_eq!(proxy.next_nonce, u64::MAX);
        assert_rejected(&mut proxy, u64::MAX, ProxyError::NonceExhausted);
    }
}
