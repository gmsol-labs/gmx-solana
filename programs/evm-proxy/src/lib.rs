use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

declare_id!("Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6");

/// Seed prefix for a permanent EVM proxy account.
#[constant]
pub const PROXY_SEED: &[u8] = b"proxy";
/// Current proxy account layout version.
pub const PROXY_VERSION: u8 = 1;
/// Length of an EVM address in bytes.
pub const EVM_ADDRESS_LEN: usize = 20;
/// Bytes reserved for future proxy account fields.
pub const PROXY_RESERVED_LEN: usize = 256;
const PROXY_PADDING_0_LEN: usize = 6;
const PROXY_PADDING_1_LEN: usize = 4;

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
    /// Explicit padding before the reserved bytes.
    pub padding_1: [u8; PROXY_PADDING_1_LEN],
    /// Future fields take bytes from this region without reallocating the account.
    pub reserved: [u8; PROXY_RESERVED_LEN],
}

impl InitSpace for Proxy {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
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
        const EXPECTED_RESERVED_OFFSET: usize = 32;
        assert_eq!(std::mem::size_of::<Proxy>(), EXPECTED_PROXY_ACCOUNT_SIZE);
        assert_eq!(Proxy::INIT_SPACE, EXPECTED_PROXY_ACCOUNT_SIZE);
        assert_eq!(
            std::mem::offset_of!(Proxy, evm_address),
            EXPECTED_EVM_ADDRESS_OFFSET
        );
        assert_eq!(
            std::mem::offset_of!(Proxy, reserved),
            EXPECTED_RESERVED_OFFSET
        );
    }
}
