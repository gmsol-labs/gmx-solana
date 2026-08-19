/// Builder for the `create_order` instruction.
pub mod create;

/// Builder for the `close_order` instruction.
pub mod close;

/// Builder for the `update_order` instruction.
pub mod update;

/// Builder for position account management instructions.
pub mod position;

/// Builder for the `set_builder_fee` instruction.
pub mod set_builder_fee;

/// Min execution lamports for order.
pub const MIN_EXECUTION_LAMPORTS_FOR_ORDER: u64 = 300_000;

pub use self::{
    close::{CloseOrder, CloseOrderHint},
    create::{
        CreateOrder, CreateOrderHint, CreateOrderKind, CreateOrderParams, DecreasePositionSwapType,
    },
    position::PreparePosition,
    set_builder_fee::{SetBuilderFee, SetBuilderFeeHint},
    update::{UpdateOrder, UpdateOrderHint, UpdateOrderParams},
};
