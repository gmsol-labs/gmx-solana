use std::ops::Deref;

use gmsol_programs::gmsol_competition::{
    client::{accounts, args},
    ID,
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

/// Operations for competition.
pub trait CompetitionOps<C> {
    /// Initialzie a competition.
    fn initialize_competition(&self, params: &CompetitionParams) -> TransactionBuilder<C, Pubkey>;

    /// Create participant account idempotently.
    fn create_participant_idempotent(
        &self,
        competition: &Pubkey,
        trader: Option<&Pubkey>,
    ) -> TransactionBuilder<C, Pubkey>;

    /// Close a participant account.
    fn close_participant(&self, competition: &Pubkey) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> CompetitionOps<C> for crate::Client<C> {
    fn initialize_competition(&self, params: &CompetitionParams) -> TransactionBuilder<C, Pubkey> {
        let payer = self.payer();
        let competition = crate::pda::find_competition_address(&payer, params.start_time, &ID).0;
        self.program(ID)
            .transaction()
            .output(competition)
            .anchor_args(args::InitializeCompetition::from(params.clone()))
            .anchor_accounts(accounts::InitializeCompetition {
                payer,
                competition,
                system_program: system_program::ID,
            })
    }

    fn create_participant_idempotent(
        &self,
        competition: &Pubkey,
        trader: Option<&Pubkey>,
    ) -> TransactionBuilder<C, Pubkey> {
        let payer = self.payer();
        let trader = trader.copied().unwrap_or(payer);
        let participant = crate::pda::find_participant_address(competition, &trader, &ID).0;
        self.program(ID)
            .transaction()
            .output(participant)
            .anchor_args(args::CreateParticipantIdempotent {})
            .anchor_accounts(accounts::CreateParticipantIdempotent {
                payer,
                competition: *competition,
                participant,
                trader,
                system_program: system_program::ID,
            })
    }

    fn close_participant(&self, competition: &Pubkey) -> TransactionBuilder<C> {
        let trader = self.payer();
        let participant = crate::pda::find_participant_address(competition, &trader, &ID).0;
        self.program(ID)
            .transaction()
            .anchor_args(args::CloseParticipant {})
            .anchor_accounts(accounts::CloseParticipant {
                trader,
                competition: *competition,
                participant,
            })
    }
}

/// Competition Params.
#[derive(Debug, Clone, typed_builder::TypedBuilder)]
pub struct CompetitionParams {
    start_time: i64,
    end_time: i64,
    volume_threshold: u128,
    extension_duration: i64,
    extension_cap: i64,
    only_count_increase: bool,
    volume_merge_window: i64,
}

impl From<CompetitionParams> for args::InitializeCompetition {
    fn from(params: CompetitionParams) -> Self {
        let CompetitionParams {
            start_time,
            end_time,
            volume_threshold,
            extension_duration,
            extension_cap,
            only_count_increase,
            volume_merge_window,
        } = params;
        Self {
            start_time,
            end_time,
            volume_threshold,
            extension_duration,
            extension_cap,
            only_count_increase,
            volume_merge_window,
        }
    }
}
