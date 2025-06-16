use anchor_lang::prelude::*;

#[error_code]
pub enum CompetitionError {
    #[msg("outside competition time")]
    OutsideCompetitionTime,
    #[msg("invalid trade event")]
    InvalidTradeEvent,
    #[msg("invalid action kind")]
    InvalidActionKind,
    #[msg("invalid time range")]
    InvalidTimeRange,
    #[msg("invalid time extension")]
    InvalidTimeExtension,
    #[msg("invalid volume threshold")]
    InvalidVolumeThreshold,
    #[msg("invalid max extension")]
    InvalidMaxExtension,
    #[msg("competition is still in progress")]
    CompetitionInProgress,
    #[msg("invalid volume merge window")]
    InvalidVolumeMergeWindow,
}
