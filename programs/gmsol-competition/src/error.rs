use anchor_lang::prelude::*;

#[error_code]
pub enum CompetitionError {
    #[msg("Competition is not active")]
    CompetitionNotActive,

    #[msg("Outside competition time window")]
    OutsideCompetitionTime,

    #[msg("Invalid time range")]
    InvalidTimeRange,

    #[msg("Invalid caller")]
    InvalidCaller,
}
