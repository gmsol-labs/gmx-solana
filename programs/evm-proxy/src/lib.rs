use anchor_lang::prelude::*;

declare_id!("Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6");

#[constant]
pub const PROXY_SEED: &[u8] = b"proxy";
pub const PROXY_VERSION: u8 = 1;

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
