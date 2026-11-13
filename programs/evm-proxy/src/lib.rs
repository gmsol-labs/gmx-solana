use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};

mod intent_verifier;

declare_id!("Dodg22E8BxfCtYBzHVHmAzFqD5bZy6q9Tp4NdYSxkmp6");

#[constant]
pub const PROXY_SEED: &[u8] = b"proxy";
pub const PROXY_VERSION: u8 = 1;
pub const PROXY_LAST_NONCE_OFFSET: usize = 8;
const PROXY_NONCE_END: usize = 16;
#[constant]
pub const CONFIG_SEED: &[u8] = b"config";
pub const MAX_ALLOWED_CHAINS: usize = 32;
pub const INTENT_KIND_CANCEL: u8 = 1;
pub const INTENT_KIND_WITHDRAW: u8 = 2;

/// Program-wide policy, initialized once by the current upgrade authority.
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

#[event]
pub struct IntentConsumed {
    pub proxy: Pubkey,
    pub nonce: u64,
    pub kind: u8,
}

/// A successful transfer from a proxy-controlled token account.
#[event]
pub struct Withdrawn {
    pub proxy: Pubkey,
    pub nonce: u64,
    pub mint: Pubkey,
    pub amount: u64,
    pub recipient: Pubkey,
}

/// Permanent account for one EVM address.
#[account(zero_copy)]
pub struct Proxy {
    pub version: u8,
    pub bump: u8,
    pub padding_0: [u8; 6],
    pub evm_address: [u8; 20],
    pub padding_1: [u8; 4],
    pub reserved: [u8; 256],
}

impl Proxy {
    pub fn validate_identity(&self, evm_address: [u8; 20], bump: u8) -> Result<()> {
        require_eq!(self.version, PROXY_VERSION, ProxyError::InvalidProxy);
        require_eq!(self.bump, bump, ProxyError::InvalidProxy);
        require!(self.evm_address == evm_address, ProxyError::InvalidProxy);
        Ok(())
    }

    pub fn consumed_nonce(&self) -> u64 {
        let mut nonce = [0; 8];
        nonce.copy_from_slice(&self.reserved[PROXY_LAST_NONCE_OFFSET..PROXY_NONCE_END]);
        u64::from_le_bytes(nonce)
    }

    pub fn expected_next_nonce(&self) -> u64 {
        self.consumed_nonce().wrapping_add(1)
    }

    pub fn consume_next_nonce(&mut self, nonce: u64) -> Result<()> {
        require_eq!(
            nonce,
            self.expected_next_nonce(),
            ProxyError::InvalidIntentNonce
        );
        self.reserved[PROXY_LAST_NONCE_OFFSET..PROXY_NONCE_END]
            .copy_from_slice(&nonce.to_le_bytes());
        Ok(())
    }
}

#[program]
pub mod gmsol_evm_proxy {
    use super::*;

    /// Bind the singleton Config to one Store and a caller-supplied genesis hash.
    /// The current program upgrade authority must sign; the program does not
    /// independently attest the supplied hash against the cluster.
    ///
    /// # Errors
    ///
    /// Anchor rejects an absent or wrong upgrade authority, unrelated ProgramData,
    /// noncanonical or existing Config, or invalid Store ownership/discriminator.
    /// Returns `InvalidAdmin` for a zero first admin and `InvalidStore` when the
    /// Store account is too short to load.
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
        config.paused = true;
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

    /// Change whether one full-width EIP-155 chain ID is allowed.
    ///
    /// # Errors
    ///
    /// Anchor rejects a missing current-admin signature or noncanonical Config.
    /// Returns `NoConfigChange` if the requested status already holds and
    /// `AllowedChainsFull` if the bounded set cannot accept another chain.
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

    /// Pause or unpause the intent policy recorded in Config.
    ///
    /// # Errors
    ///
    /// Anchor rejects a missing current-admin signature or noncanonical Config.
    /// Returns `NoConfigChange` when the requested pause state already holds.
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

    /// Nominate or replace the next admin without transferring current control.
    ///
    /// # Errors
    ///
    /// Anchor rejects a missing current-admin signature or noncanonical Config.
    /// Returns `InvalidAdmin` for the zero address or `NoConfigChange` when
    /// nominating the current or already pending admin.
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

    /// Complete a nominated admin handover with the nominee's signature.
    ///
    /// # Errors
    ///
    /// Anchor rejects a missing signer or noncanonical Config. Returns
    /// `NotPendingAdmin` when the signer is not the nominated admin.
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

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }

    /// Create the permanent proxy for an EVM address; the payer gains no authority.
    ///
    /// # Errors
    ///
    /// Fails if account creation or PDA validation fails, or if an existing proxy
    /// has a different version, bump, or EVM address.
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
        proxy.validate_identity(evm_address, ctx.bumps.proxy)
    }

    /// Consume the next signed Cancel nonce, wrapping from u64::MAX to zero,
    /// without changing the proxy's business state.
    ///
    /// # Errors
    ///
    /// Anchor rejects a missing submitter signature, invalid account ownership or
    /// discriminators, noncanonical Config or Proxy PDAs, or a readonly Proxy.
    /// Account loading/borrowing and Clock access errors propagate.
    /// Returns `InvalidProxy` for a stored version, bump or EVM address mismatch,
    /// and `InvalidIntentNonce` when the signed nonce is not the next nonce.
    /// Returns `Paused` while Config is paused and `ChainNotAllowed` when the
    /// signed chain ID is not allowed by Config.
    /// Returns `InvalidDeadline` for a negative Clock timestamp or a deadline
    /// outside the inclusive window from now through now plus 3600 seconds.
    /// Returns `InvalidSignature` for an invalid recovery ID, zero or high `s`,
    /// failed recovery, or a recovered address that does not match the EVM owner
    /// for the computed domain and Cancel digest.
    pub fn cancel(
        ctx: Context<Cancel>,
        evm_address: [u8; 20],
        chain_id: [u8; 32],
        nonce: u64,
        deadline: u64,
        signature: [u8; 64],
        recovery_id: u8,
    ) -> Result<()> {
        let proxy_key = ctx.accounts.proxy.key();
        {
            let proxy = ctx.accounts.proxy.load()?;
            proxy.validate_identity(evm_address, ctx.bumps.proxy)?;
            require!(
                nonce == proxy.expected_next_nonce(),
                ProxyError::InvalidIntentNonce
            );
        }

        let struct_hash = intent_verifier::cancel_struct_hash(&proxy_key, nonce, deadline);
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
        emit!(IntentConsumed {
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
    /// # Errors
    /// Returns `ZeroAmount` for zero, `SelfRecipient` when the recipient wallet
    /// is the proxy, and `InvalidDecimals` for a mint decimal mismatch. Account
    /// constraints reject noncanonical or missing ATAs, wrong mint or Token
    /// program. The checked token transfer rejects frozen token accounts.
    /// Identity and nonce checks reject a
    /// wrong proxy or consumed nonce. Verification rejects an expired deadline,
    /// disallowed chain, paused Config, or invalid EVM signature. A failed
    /// transfer leaves the nonce unconsumed and token balances unchanged.
    pub fn withdraw(
        ctx: Context<Withdraw>,
        evm_address: [u8; 20],
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
        {
            let proxy = ctx.accounts.proxy.load()?;
            proxy.validate_identity(evm_address, ctx.bumps.proxy)?;
            require!(
                nonce == proxy.expected_next_nonce(),
                ProxyError::InvalidIntentNonce
            );
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
        let bump = [ctx.bumps.proxy];
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
        ctx.accounts.proxy.load_mut()?.consume_next_nonce(nonce)?;
        emit!(IntentConsumed {
            proxy: proxy_key,
            nonce,
            kind: INTENT_KIND_WITHDRAW,
        });
        emit!(Withdrawn {
            proxy: proxy_key,
            nonce,
            mint,
            amount,
            recipient
        });
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
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
    /// Pays rent but has no authority over the proxy.
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

#[derive(Accounts)]
#[instruction(evm_address: [u8; 20])]
pub struct Cancel<'info> {
    pub submitter: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [PROXY_SEED, evm_address.as_ref()], bump)]
    pub proxy: AccountLoader<'info, Proxy>,
}

#[derive(Accounts)]
#[instruction(evm_address: [u8; 20])]
pub struct Withdraw<'info> {
    pub submitter: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [PROXY_SEED, evm_address.as_ref()], bump)]
    pub proxy: AccountLoader<'info, Proxy>,
    pub mint: Account<'info, Mint>,
    /// CHECK: the key is signed in the Withdraw digest; its canonical ATA is checked below.
    pub recipient: UncheckedAccount<'info>,
    #[account(mut, associated_token::mint = mint, associated_token::authority = proxy)]
    pub source: Account<'info, TokenAccount>,
    #[account(mut, associated_token::mint = mint, associated_token::authority = recipient)]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
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
    #[msg("Intent nonce is not the proxy's next nonce")]
    InvalidIntentNonce,
    // Preserve the numeric slot so later verification errors keep their codes.
    #[msg("Reserved proxy intent error")]
    ReservedIntentError,
    #[msg("Intent verification is paused")]
    Paused,
    #[msg("EVM chain is not allowed")]
    ChainNotAllowed,
    #[msg("Intent deadline is outside the execution window")]
    InvalidDeadline,
    #[msg("Invalid EVM signature")]
    InvalidSignature,
    #[msg("Withdraw amount must be positive")]
    ZeroAmount,
    #[msg("Proxy cannot withdraw to itself")]
    SelfRecipient,
    #[msg("Signed decimals differ from the mint")]
    InvalidDecimals,
}

#[cfg(not(feature = "no-entrypoint"))]
gmsol_utils::security_txt!("GMX-Solana EVM Proxy Program");

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    fn proxy_with_nonce(nonce: u64) -> Proxy {
        let mut proxy = Proxy::zeroed();
        proxy.reserved.fill(0xa5);
        proxy.reserved[PROXY_LAST_NONCE_OFFSET..PROXY_NONCE_END]
            .copy_from_slice(&nonce.to_le_bytes());
        proxy
    }

    #[test]
    fn proxy_account_binary_layout() {
        assert_eq!(std::mem::size_of::<Proxy>(), 288);
        assert_eq!(std::mem::offset_of!(Proxy, evm_address), 8);
        assert_eq!(std::mem::offset_of!(Proxy, reserved), 32);
        assert_eq!(8 + std::mem::offset_of!(Proxy, reserved), 40);
        assert_eq!(
            8 + std::mem::offset_of!(Proxy, reserved) + PROXY_LAST_NONCE_OFFSET,
            48
        );
        assert_eq!(
            8 + std::mem::offset_of!(Proxy, reserved) + PROXY_NONCE_END,
            56
        );
    }

    #[test]
    fn first_and_ordinary_nonce_preserve_all_other_bytes() {
        let mut proxy = proxy_with_nonce(0);
        assert_eq!(proxy.consumed_nonce(), 0);
        for nonce in [1_u64, 2] {
            let mut expected = bytemuck::bytes_of(&proxy).to_vec();
            expected[40..48].copy_from_slice(&nonce.to_le_bytes());
            assert_eq!(proxy.expected_next_nonce(), nonce);
            proxy.consume_next_nonce(nonce).unwrap();
            assert_eq!(proxy.consumed_nonce(), nonce);
            assert_eq!(bytemuck::bytes_of(&proxy), expected.as_slice());
        }
    }

    #[test]
    fn wrong_and_repeated_nonces_cannot_change_state() {
        let mut proxy = proxy_with_nonce(0);
        for wrong in [0, 2, u64::MAX] {
            let before = bytemuck::bytes_of(&proxy).to_vec();
            let Error::AnchorError(error) = proxy.consume_next_nonce(wrong).unwrap_err() else {
                panic!("expected nonce mismatch");
            };
            assert_eq!(error.error_code_number, 6006);
            assert_eq!(bytemuck::bytes_of(&proxy), before.as_slice());
        }
        proxy.consume_next_nonce(1).unwrap();
        let before = bytemuck::bytes_of(&proxy).to_vec();
        assert!(proxy.consume_next_nonce(1).is_err());
        assert_eq!(bytemuck::bytes_of(&proxy), before.as_slice());
    }

    #[test]
    fn max_nonce_wraps_to_zero_then_one_without_other_writes() {
        let mut proxy = proxy_with_nonce(u64::MAX - 1);
        for nonce in [u64::MAX, 0, 1] {
            let mut expected = bytemuck::bytes_of(&proxy).to_vec();
            expected[40..48].copy_from_slice(&nonce.to_le_bytes());
            assert_eq!(proxy.expected_next_nonce(), nonce);
            proxy.consume_next_nonce(nonce).unwrap();
            assert_eq!(proxy.consumed_nonce(), nonce);
            assert_eq!(bytemuck::bytes_of(&proxy), expected.as_slice());
            let before = bytemuck::bytes_of(&proxy).to_vec();
            assert!(proxy.consume_next_nonce(nonce).is_err());
            assert_eq!(bytemuck::bytes_of(&proxy), before.as_slice());
        }
    }

    #[test]
    fn verification_error_numbers_remain_stable() {
        for (error, code) in [
            (ProxyError::InvalidIntentNonce, 6006),
            (ProxyError::ReservedIntentError, 6007),
            (ProxyError::Paused, 6008),
            (ProxyError::ChainNotAllowed, 6009),
            (ProxyError::InvalidDeadline, 6010),
            (ProxyError::InvalidSignature, 6011),
        ] {
            let Error::AnchorError(error) = anchor_lang::error!(error) else {
                panic!("expected Anchor error");
            };
            assert_eq!(error.error_code_number, code);
        }
    }
}
