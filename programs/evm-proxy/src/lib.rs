use anchor_lang::prelude::*;

declare_id!("Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6");

#[constant]
pub const PROXY_SEED: &[u8] = b"proxy";
pub const PROXY_VERSION: u8 = 1;
#[constant]
pub const CONFIG_SEED: &[u8] = b"config";
pub const MAX_ALLOWED_CHAINS: usize = 32;
const INITIAL_PAUSED: bool = true;

/// Program-wide intent policy.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub store: Pubkey,
    pub genesis_hash: [u8; 32],
    pub admin: Pubkey,
    pub pending_admin: Option<Pubkey>,
    pub paused: bool,
    #[max_len(MAX_ALLOWED_CHAINS)]
    pub allowed_chain_ids: Vec<[u8; 32]>,
}

#[event]
pub struct ConfigCreated {
    pub config: Pubkey,
    pub store: Pubkey,
    pub genesis_hash: [u8; 32],
    pub admin: Pubkey,
    pub paused: bool,
    pub allowed_chain_ids: Vec<[u8; 32]>,
}

#[event]
pub struct ConfigChainAllowedChanged {
    pub config: Pubkey,
    pub chain_id: [u8; 32],
    pub allowed: bool,
}

#[event]
pub struct ConfigPauseChanged {
    pub config: Pubkey,
    pub previous_paused: bool,
    pub paused: bool,
}

#[event]
pub struct ConfigAdminProposed {
    pub config: Pubkey,
    pub admin: Pubkey,
    pub previous_pending_admin: Option<Pubkey>,
    pub next_admin: Pubkey,
}

#[event]
pub struct ConfigAdminAccepted {
    pub config: Pubkey,
    pub previous_admin: Pubkey,
    pub admin: Pubkey,
    pub pending_admin: Option<Pubkey>,
}

/// One permanent account for a 20-byte EVM address.
#[account(zero_copy)]
pub struct Proxy {
    pub version: u8,
    pub bump: u8,
    pub padding_0: [u8; 6],
    pub evm_address: [u8; 20],
    pub padding_1: [u8; 4],
    pub reserved: [u8; 256],
}

#[program]
pub mod gmsol_evm_proxy {
    use super::*;

    /// Initialize the singleton Config under the current program upgrade authority.
    pub fn create_config(
        ctx: Context<CreateConfig>,
        genesis_hash: [u8; 32],
        first_admin: Pubkey,
    ) -> Result<()> {
        require_keys_neq!(first_admin, Pubkey::default(), ProxyError::InvalidAdmin);
        require!(
            ctx.accounts.store.to_account_info().data_len()
                >= 8 + std::mem::size_of::<gmsol_store::states::Store>(),
            ProxyError::InvalidStore
        );
        ctx.accounts.store.load()?;
        let config = &mut ctx.accounts.config;
        config.store = ctx.accounts.store.key();
        config.genesis_hash = genesis_hash;
        config.admin = first_admin;
        config.pending_admin = None;
        config.paused = INITIAL_PAUSED;
        config.allowed_chain_ids = Vec::new();
        emit!(ConfigCreated {
            config: config.key(),
            store: config.store,
            genesis_hash,
            admin: first_admin,
            paused: config.paused,
            allowed_chain_ids: config.allowed_chain_ids.clone(),
        });
        Ok(())
    }

    /// Set whether one full-width EIP-155 chain ID is accepted by Config.
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
        emit!(ConfigChainAllowedChanged {
            config: config.key(),
            chain_id,
            allowed,
        });
        Ok(())
    }

    /// Pause or unpause every intent governed by Config.
    pub fn set_paused(ctx: Context<CurrentConfigAdmin>, paused: bool) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(config.paused != paused, ProxyError::NoConfigChange);
        let previous_paused = config.paused;
        config.paused = paused;
        emit!(ConfigPauseChanged {
            config: config.key(),
            previous_paused,
            paused,
        });
        Ok(())
    }

    /// Propose or replace an unaccepted next admin.
    pub fn propose_admin(ctx: Context<CurrentConfigAdmin>, next_admin: Pubkey) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require_keys_neq!(next_admin, Pubkey::default(), ProxyError::InvalidAdmin);
        require_keys_neq!(next_admin, config.admin, ProxyError::NoConfigChange);
        require!(
            config.pending_admin != Some(next_admin),
            ProxyError::NoConfigChange
        );
        let previous_pending_admin = config.pending_admin;
        config.pending_admin = Some(next_admin);
        emit!(ConfigAdminProposed {
            config: config.key(),
            admin: config.admin,
            previous_pending_admin,
            next_admin,
        });
        Ok(())
    }

    /// Complete a pending handover when the nominated admin signs.
    pub fn accept_admin(ctx: Context<AcceptConfigAdmin>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(
            config.pending_admin == Some(ctx.accounts.next_admin.key()),
            ProxyError::NotPendingAdmin
        );
        let previous_admin = config.admin;
        config.admin = ctx.accounts.next_admin.key();
        config.pending_admin = None;
        emit!(ConfigAdminAccepted {
            config: config.key(),
            previous_admin,
            admin: config.admin,
            pending_admin: config.pending_admin,
        });
        Ok(())
    }

    /// Create a proxy for an EVM address. Anyone may pay; the payer gains no authority.
    ///
    /// # Errors
    ///
    /// Returns an error if account initialization fails or an existing proxy has a
    /// different version, bump, or EVM address.
    pub fn create_proxy(ctx: Context<CreateProxy>, evm_address: [u8; 20]) -> Result<()> {
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

#[derive(Accounts)]
pub struct CreateConfig<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub upgrade_authority: Signer<'info>,
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub config: Account<'info, Config>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()))]
    pub program: Program<'info, program::GmsolEvmProxy>,
    #[account(constraint = program_data.upgrade_authority_address == Some(upgrade_authority.key()))]
    pub program_data: Account<'info, ProgramData>,
    pub store: AccountLoader<'info, gmsol_store::states::Store>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CurrentConfigAdmin<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump, has_one = admin)]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct AcceptConfigAdmin<'info> {
    pub next_admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
#[instruction(evm_address: [u8; 20])]
pub struct CreateProxy<'info> {
    /// Pays account rent only.
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + std::mem::size_of::<Proxy>(),
        seeds = [PROXY_SEED, evm_address.as_ref()],
        bump,
    )]
    pub proxy: AccountLoader<'info, Proxy>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum ProxyError {
    #[msg("Proxy account data does not match its EVM address")]
    InvalidProxy,
    #[msg("Store account data is too short")]
    InvalidStore,
    #[msg("Admin address cannot be zero")]
    InvalidAdmin,
    #[msg("Config change has no effect")]
    NoConfigChange,
    #[msg("Allowed EVM chain set is full")]
    AllowedChainsFull,
    #[msg("Signer is not the pending admin")]
    NotPendingAdmin,
}

#[cfg(not(feature = "no-entrypoint"))]
gmsol_utils::security_txt!("GMX-Solana EVM Proxy Program");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_layout() {
        assert_eq!(std::mem::size_of::<Proxy>(), 288);
        assert_eq!(std::mem::offset_of!(Proxy, evm_address), 8);
        assert_eq!(std::mem::offset_of!(Proxy, reserved), 32);
    }
}
