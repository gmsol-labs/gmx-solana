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

/// EVM proxy instructions.
#[program]
pub mod gmsol_evm_proxy {
    use super::*;

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
