pub use gmsol_store::{events::*, instruction as store_instruction, states::*};

/// Re-export [`gmsol_treasury`] types.
pub mod treasury {
    pub use gmsol_treasury::{instruction, states::*};
}

/// Re-export [`gmsol_timelock`] types.
pub mod timelock {
    pub use gmsol_timelock::{instruction, states::*};
}
