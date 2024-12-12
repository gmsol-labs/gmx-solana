/// States.
pub mod states;

/// Instructions.
pub mod instructions;

/// Roles.
pub mod roles;

use anchor_lang::prelude::*;
use instructions::*;

declare_id!("GTtRSYha5h8S26kPFHgYKUf8enEgabkTFwW7UToXAHoY");

#[program]
pub mod gmsol_treasury {
    use super::*;

    /// Initialize a treasury config.
    pub fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
        instructions::initialize_config(ctx)
    }
}
