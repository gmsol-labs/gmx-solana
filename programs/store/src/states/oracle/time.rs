use anchor_lang::prelude::*;

use crate::{CoreError, CoreResult};

use super::Oracle;

/// Validate Oracle Time.
pub trait ValidateOracleTime {
    /// Oracle must be updated after this time.
    fn oracle_updated_after(&self) -> CoreResult<Option<i64>>;

    /// Oracle must be updated before this time.
    fn oracle_updated_before(&self) -> CoreResult<Option<i64>>;

    /// Oracle must be updated after this slot.
    fn oracle_updated_after_slot(&self) -> CoreResult<Option<u64>>;
}

/// Extension trait for [`ValidateOracleTime`].
pub trait ValidateOracleTimeExt: ValidateOracleTime {
    /// Validate min oracle ts.
    fn validate_min_oracle_ts(&self, oracle: &Oracle) -> CoreResult<()> {
        let Some(after) = self.oracle_updated_after()? else {
            return Ok(());
        };
        if oracle.min_oracle_ts < after {
            msg!("oracle = {}, require >= {}", oracle.min_oracle_ts, after);
            return Err(CoreError::OracleTimestampsAreSmallerThanRequired);
        }
        Ok(())
    }

    /// Validate max oracle ts.
    fn validate_max_oracle_ts(&self, oracle: &Oracle) -> CoreResult<()> {
        let Some(before) = self.oracle_updated_before()? else {
            return Ok(());
        };
        if before < oracle.max_oracle_ts {
            msg!("oracle = {}, require <= {}", oracle.max_oracle_ts, before);
            return Err(CoreError::OracleTimestampsAreLargerThanRequired);
        }
        Ok(())
    }

    /// Validate min oracle updated slot.
    fn validate_min_oracle_slot(&self, oracle: &Oracle) -> CoreResult<()> {
        let Some(min_slot) = oracle.min_oracle_slot() else {
            return Err(CoreError::OracleNotUpdated);
        };
        let Some(after) = self.oracle_updated_after_slot()? else {
            return Ok(());
        };
        if min_slot < after {
            msg!("oracle = {}, require >= {}", min_slot, after);
            return Err(CoreError::InvalidOracleSlot);
        }
        Ok(())
    }
}

impl<T: ValidateOracleTime> ValidateOracleTimeExt for T {}
