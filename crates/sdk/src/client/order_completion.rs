//! The result of waiting for an order to finish, see [`Client::complete_order`].
//!
//! Owns: the [`OrderCompletion`] outcome type and how it is read from store CPI events.
//! Does NOT own: subscribing or fetching history (see `Client::complete_order_with_config`).
//!
//! [`Client::complete_order`]: crate::Client::complete_order

use gmsol_decode::gmsol::programs::GMSOLCPIEvent;
use gmsol_programs::gmsol_store::events::{OrderRemoved, TradeEvent};

/// How an order finished.
#[derive(Debug, Clone)]
pub enum OrderCompletion {
    /// The order was executed and produced this trade.
    Trade(Box<TradeEvent>),
    /// The order was closed without a trade, e.g. a swap or a cancelled order.
    NoTrade(OrderRemoved),
    /// The order is gone, but its event history does not show what happened.
    Inconclusive {
        /// The close event, if one was found.
        removed: Option<OrderRemoved>,
        /// Why the result could not be established.
        reason: String,
    },
}

/// Read the outcome from the events of a single transaction.
pub(crate) fn completion_from_events(events: Vec<GMSOLCPIEvent>) -> OrderCompletion {
    let mut removed = None;
    for event in events {
        match event {
            GMSOLCPIEvent::TradeEvent(trade) => return OrderCompletion::Trade(Box::new(trade)),
            GMSOLCPIEvent::OrderRemoved(event) => removed = Some(event),
            _ => {}
        }
    }
    match removed {
        Some(removed) => OrderCompletion::NoTrade(removed),
        None => OrderCompletion::Inconclusive {
            removed: None,
            reason: "no `OrderRemoved` event found".to_owned(),
        },
    }
}
