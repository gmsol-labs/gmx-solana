/// Max number of user flags.
pub const MAX_USER_FLAGS: usize = 8;

/// User flags.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
#[non_exhaustive]
pub enum UserFlag {
    /// Is initialized.
    Initialized,
    // CHECK: should have no more than `MAX_USER_FLAGS` of flags.
}
