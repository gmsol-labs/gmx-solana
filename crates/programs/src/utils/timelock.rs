#[cfg(feature = "gmsol-utils")]
mod utils {
    use gmsol_utils::fixed_str::bytes_to_fixed_str;

    use crate::gmsol_timelock::accounts::Executor;

    impl Executor {
        /// Get role name.
        pub fn role_name(&self) -> crate::Result<&str> {
            bytes_to_fixed_str(&self.role_name).map_err(crate::Error::custom)
        }
    }
}
