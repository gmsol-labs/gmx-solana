use anchor_lang::{
    error::Error,
    solana_program::{clock::Clock, sysvar::Sysvar},
};

/// Clock-related operations.
pub struct AsClockMut<'a> {
    last: &'a mut i64,
}

impl AsClockMut<'_> {
    /// Just passed in seconds.
    pub fn just_passed_in_seconds(&mut self) -> gmsol_model::Result<u64> {
        let current = Clock::get().map_err(Error::from)?.unix_timestamp;
        let duration = current.saturating_sub(*self.last);
        if duration > 0 {
            *self.last = current;
            Ok(duration as u64)
        } else {
            Ok(0)
        }
    }
}

impl<'a> From<&'a mut i64> for AsClockMut<'a> {
    fn from(last: &'a mut i64) -> Self {
        Self { last }
    }
}

/// Clock-related operations.
pub struct AsClock<'a> {
    last: &'a i64,
}

impl AsClock<'_> {
    /// Passed in seconds.
    pub fn passed_in_seconds(&self) -> gmsol_model::Result<u64> {
        let current = Clock::get().map_err(Error::from)?.unix_timestamp;
        let duration = current.saturating_sub(*self.last);
        if duration > 0 {
            Ok(duration as u64)
        } else {
            Ok(0)
        }
    }
}

impl<'a> From<&'a i64> for AsClock<'a> {
    fn from(last: &'a i64) -> Self {
        Self { last }
    }
}
