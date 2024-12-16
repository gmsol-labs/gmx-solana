pub use gmsol_store::{events::*, instruction as store_instruction, states::*};

/// Re-export [`gmsol_treasury`].
pub mod treasury {
    pub use gmsol_treasury::{instruction, states::*};
}
