use anchor_lang::prelude::*;

declare_id!("2AxuNr6euZPKQbTwNsLBjzFTZFAevA85F4PW9m9Dv8pc");

#[program]
pub mod gmsol_competition {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
