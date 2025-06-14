use anchor_lang::prelude::*;

#[error_code]
pub enum CompetitionError {
    #[msg("Outside competition time")]
    OutsideCompetitionTime,
    #[msg("Invalid trade event")]
    InvalidTradeEvent,
    #[msg("Invalid action kind")]
    InvalidActionKind,
    #[msg("Invalid time range")]
    InvalidTimeRange,
    #[msg("Invalid time extension")]
    InvalidTimeExtension,
    #[msg("Invalid volume threshold")]
    InvalidVolumeThreshold,
    #[msg("Invalid max extension")]
    InvalidMaxExtension,
    #[msg("Competition is still in progress")]
    CompetitionInProgress,
    #[msg("Invalid volume merge window")]
    InvalidVolumeMergeWindow,
}
