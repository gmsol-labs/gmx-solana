//! The result of waiting for an order to finish, see [`Client::complete_order`].
//!
//! Owns: the [`OrderCompletion`] outcome and its [`InconclusiveReason`], `LifecycleScan`,
//! which rebuilds the outcome from an order's event history, the history re-read timing
//! ([`DEFAULT_HISTORY_WAIT`]) and the "order not found" rule (`reject_unseen_order`). Does NOT
//! own: subscribing or fetching history (see `Client::complete_order_with_config`).
//!
//! Execution and close can land in different transactions, e.g. execute at slot 100
//! (`TradeEvent`), settle the builder fee at 101, close at 102 (`OrderRemoved`). So the
//! scan walks the order's transactions newest first, from its `OrderRemoved` back to its
//! `TradeEvent`. With no trade, it walks on to the start of that order's lifecycle to
//! prove there was none.
//!
//! Invariants:
//!   - events are fed newest first: transactions newest first, and each transaction's
//!     events in reverse emission order
//!   - an order address can be reused (it is a PDA of `(store, owner, nonce)`), so a
//!     lifecycle starts at its create (`OrderUpdated { is_create: true }`) or right after an
//!     older close at the same address
//!   - an edit (`update_order`) gives the same order a new action id, so a different id on an
//!     edit is NOT a new lifecycle; the order is ours if its expected id shows up anywhere in
//!     the lifecycle (close, edit or create)
//!   - an order executes at most once (`Pending -> Completed`), so the first `TradeEvent`
//!     before any boundary is this order's; an older order's trade sits behind its own close
//!
//! [`Client::complete_order`]: crate::Client::complete_order
//! [`OrderCompletion`]: crate::client::order_completion::OrderCompletion
//! [`InconclusiveReason`]: crate::client::order_completion::InconclusiveReason
//! [`DEFAULT_HISTORY_WAIT`]: crate::client::order_completion::DEFAULT_HISTORY_WAIT

use gmsol_decode::gmsol::programs::GMSOLCPIEvent;
use gmsol_programs::gmsol_store::{
    events::{OrderRemoved, TradeEvent},
    types::{ActionState, OrderKind},
};
use solana_sdk::pubkey::Pubkey;
use std::time::Duration;

/// How an order finished.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
        reason: InconclusiveReason,
    },
}

/// Why an [`OrderCompletion::Inconclusive`] result could not be established.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InconclusiveReason {
    /// The order account is gone, but its history has no close for it. The close must exist,
    /// so the history is most likely not indexed yet: asking again later can succeed.
    CloseNotFound,
    /// A completed position order has no `TradeEvent` in its history. The trade must exist,
    /// so the history is most likely not indexed yet: asking again later can succeed.
    TradeNotFound,
    /// The newest close at this address belongs to another order (the address was reused).
    OtherOrderClosed {
        /// Action id of the close that was found.
        found_id: u64,
        /// Action id of the order being waited on.
        expected_id: u64,
    },
    /// The start of the order's lifecycle was not found within the scan limit.
    ScanLimitReached,
    /// An older transaction in the order's history could not be fetched, e.g. an RPC timeout
    /// or rate limit. Asking again later can succeed.
    RpcFailed(String),
    /// An older transaction in the order's history holds an event that can't be decoded, e.g.
    /// one from an earlier program version. Asking again gives the same answer.
    DecodeFailed(String),
}

impl InconclusiveReason {
    /// Whether asking again later can change the answer: a close or trade that must exist is
    /// not indexed yet, or a fetch failed (timeout, rate limit). `complete_order` already
    /// retries these within its history wait; a `true` here means it ran out of time.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::CloseNotFound | Self::TradeNotFound | Self::RpcFailed(_)
        )
    }
}

impl std::fmt::Display for InconclusiveReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CloseNotFound => write!(f, "no `OrderRemoved` event found for the order"),
            Self::TradeNotFound => write!(f, "order completed but no `TradeEvent` was found"),
            Self::OtherOrderClosed {
                found_id,
                expected_id,
            } => write!(
                f,
                "latest close is for action id {found_id}, expected {expected_id}"
            ),
            Self::ScanLimitReached => write!(
                f,
                "lifecycle start not found within {MAX_SCANNED_TXS} transactions"
            ),
            Self::RpcFailed(err) => write!(f, "failed to fetch older history: {err}"),
            Self::DecodeFailed(err) => write!(f, "failed to decode older history: {err}"),
        }
    }
}

/// Wait before the first history re-read while the close or trade is not indexed yet. Each
/// later wait doubles (1 s, 2 s, 4 s, ...), so a slow index costs a few reads, not one per second.
pub(crate) const HISTORY_RETRY_FIRST_DELAY: Duration = Duration::from_secs(1);

/// Default for how long [`Client::complete_order`] keeps re-reading history while the close
/// or trade is not indexed yet: reads at 0, 1, 3 and 5 s. Pass another limit to
/// [`Client::complete_order_with_config`].
///
/// [`Client::complete_order`]: crate::Client::complete_order
/// [`Client::complete_order_with_config`]: crate::Client::complete_order_with_config
// The index usually lags by a second or two; there is no fixed bound, so give up eventually.
pub const DEFAULT_HISTORY_WAIT: Duration = Duration::from_secs(5);

/// Most event-bearing transactions read before giving up on finding the lifecycle start.
// An order's own history is usually 2–4 transactions (create, execute, settle, close).
pub(crate) const MAX_SCANNED_TXS: usize = 32;

/// Rebuilds one order lifecycle's outcome from its events, fed newest first.
///
/// ```text
/// tx 102: [OrderRemoved{order: O, id: 7}]      -> removed found
/// tx 101: [BuilderFeeSettled]                  -> ignored
/// tx 100: [TradeEvent{order: O}]               -> trade found, done
/// finish() -> OrderCompletion::Trade(..)
/// ```
pub(crate) struct LifecycleScan {
    order: Pubkey,
    /// Action id of the order when it was seen open, if it was.
    expected_id: Option<u64>,
    removed: Option<OrderRemoved>,
    trade: Option<TradeEvent>,
    reached_start: bool,
    /// Whether the lifecycle carries `expected_id` (always true when there is none).
    id_confirmed: bool,
    scanned_txs: usize,
}

impl LifecycleScan {
    pub(crate) fn new(order: Pubkey, expected_id: Option<u64>) -> Self {
        Self {
            order,
            expected_id,
            removed: None,
            trade: None,
            reached_start: false,
            id_confirmed: expected_id.is_none(),
            scanned_txs: 0,
        }
    }

    /// Feed one transaction's events in emission order. Returns `true` once nothing older
    /// can change the outcome.
    pub(crate) fn push_transaction(&mut self, events: &[GMSOLCPIEvent]) -> bool {
        self.scanned_txs += 1;
        for event in events.iter().rev() {
            if self.push_event(event) {
                return true;
            }
        }
        self.scanned_txs >= MAX_SCANNED_TXS
    }

    /// Returns `true` when the scan is done.
    fn push_event(&mut self, event: &GMSOLCPIEvent) -> bool {
        let Some(removed) = &self.removed else {
            // Events newer than the close belong to a later lifecycle; skip them.
            if let GMSOLCPIEvent::OrderRemoved(event) = event {
                if event.order == self.order {
                    self.confirm_id(event.id);
                    self.removed = Some(event.clone());
                }
            }
            return false;
        };
        match event {
            GMSOLCPIEvent::TradeEvent(trade)
                if trade.order == self.order && trade.store == removed.store =>
            {
                self.trade.get_or_insert(*trade);
            }
            GMSOLCPIEvent::OrderRemoved(event) if event.order == self.order => {
                // The same close seen twice (live, then again in history) is not a boundary.
                let same_close = event.id == removed.id && event.slot == removed.slot;
                self.reached_start = !same_close;
            }
            GMSOLCPIEvent::OrderUpdated(event) if event.order == self.order => {
                self.confirm_id(event.id);
                self.reached_start = event.is_create;
            }
            _ => {}
        }
        // A trade is only returned once the lifecycle is known to be the expected order's.
        self.reached_start || (self.trade.is_some() && self.id_confirmed)
    }

    fn confirm_id(&mut self, id: u64) {
        if self.expected_id == Some(id) {
            self.id_confirmed = true;
        }
    }

    /// Whether this order's close has been found.
    pub(crate) fn has_close(&self) -> bool {
        self.removed.is_some()
    }

    /// The outcome when an older transaction could not be read. Only used once the close is
    /// found: before that, the read error itself is the answer.
    pub(crate) fn finish_with_read_error(self, err: &crate::Error) -> OrderCompletion {
        // Decoder errors come through as `Error::Decode`; fetch errors as `Error::Custom`.
        let reason = match err {
            crate::Error::Decode(_) => InconclusiveReason::DecodeFailed(err.to_string()),
            _ => InconclusiveReason::RpcFailed(err.to_string()),
        };
        OrderCompletion::Inconclusive {
            removed: self.removed,
            reason,
        }
    }

    /// The outcome. `history_ended` is `true` when there were no older transactions, so the
    /// oldest one read is the start of the address's history.
    pub(crate) fn finish(self, history_ended: bool) -> OrderCompletion {
        let inconclusive = |removed, reason| OrderCompletion::Inconclusive { removed, reason };
        let Some(removed) = self.removed else {
            return inconclusive(None, InconclusiveReason::CloseNotFound);
        };
        let lifecycle_read = self.reached_start || history_ended;
        if !self.id_confirmed {
            // `removed` is not reported: it may be another order's close, and callers read it
            // as theirs.
            let reason = match (lifecycle_read, self.expected_id) {
                (true, Some(expected_id)) => InconclusiveReason::OtherOrderClosed {
                    found_id: removed.id,
                    expected_id,
                },
                _ => InconclusiveReason::ScanLimitReached,
            };
            return inconclusive(None, reason);
        }
        if let Some(trade) = self.trade {
            return OrderCompletion::Trade(Box::new(trade));
        }
        if !lifecycle_read {
            return inconclusive(Some(removed), InconclusiveReason::ScanLimitReached);
        }
        if expects_trade(&removed) {
            return inconclusive(Some(removed), InconclusiveReason::TradeNotFound);
        }
        OrderCompletion::NoTrade(removed)
    }
}

/// Turns "no close in history" into a not-found error for an order that was never seen open.
///
/// Waiting before the create lands looks the same as waiting after the close: no account.
/// Without a sighting of the account and without a close in history, the order most likely
/// doesn't exist yet, and calling it closed (`Inconclusive`) would be wrong.
pub(crate) fn reject_unseen_order(
    order: &Pubkey,
    seen_open: bool,
    completion: OrderCompletion,
) -> crate::Result<OrderCompletion> {
    match completion {
        OrderCompletion::Inconclusive {
            reason: InconclusiveReason::CloseNotFound,
            ..
        } if !seen_open => Err(crate::Error::custom(format!(
            "order {order} not found: no account and no close in its history (not created yet?)"
        ))),
        completion => Ok(completion),
    }
}

/// Executed position orders always emit a `TradeEvent`; swaps and cancelled orders don't.
fn expects_trade(removed: &OrderRemoved) -> bool {
    matches!(removed.state, ActionState::Completed)
        && !matches!(removed.kind, OrderKind::MarketSwap | OrderKind::LimitSwap)
}

#[cfg(test)]
mod tests {
    use gmsol_programs::{
        anchor_lang::AnchorDeserialize,
        gmsol_store::{
            events::OrderUpdated,
            types::{DecreasePositionSwapType, OrderParamsForEvent},
        },
    };

    use super::*;

    const STORE: Pubkey = Pubkey::new_from_array([1; 32]);
    const ORDER: Pubkey = Pubkey::new_from_array([2; 32]);
    const OTHER: Pubkey = Pubkey::new_from_array([3; 32]);

    fn trade(order: Pubkey, trade_id: u64) -> GMSOLCPIEvent {
        // Every `TradeEvent` field is a number or a key, so all-zero bytes decode.
        let mut event = TradeEvent::deserialize(&mut &[0u8; 4096][..]).unwrap();
        event.store = STORE;
        event.order = order;
        event.trade_id = trade_id;
        GMSOLCPIEvent::TradeEvent(event)
    }

    fn removed_event(id: u64, kind: OrderKind, state: ActionState) -> OrderRemoved {
        OrderRemoved {
            id,
            ts: 0,
            slot: 0,
            store: STORE,
            order: ORDER,
            kind,
            market_token: Pubkey::default(),
            owner: Pubkey::default(),
            state,
            reason: "executed".to_owned(),
        }
    }

    fn removed(id: u64) -> GMSOLCPIEvent {
        GMSOLCPIEvent::OrderRemoved(removed_event(
            id,
            OrderKind::MarketIncrease,
            ActionState::Completed,
        ))
    }

    fn created(id: u64) -> GMSOLCPIEvent {
        updated(id, true)
    }

    fn updated(id: u64, is_create: bool) -> GMSOLCPIEvent {
        let params = OrderParamsForEvent {
            kind: OrderKind::MarketIncrease,
            is_long: true,
            decrease_position_swap_type: DecreasePositionSwapType::NoSwap,
            position: None,
            collateral_token: Pubkey::default(),
            initial_collateral_token: None,
            initial_collateral_delta_amount: 0,
            size_delta_value: 0,
            min_output: 0,
            trigger_price: 0,
            acceptable_price: 0,
            valid_from_ts: 0,
        };
        GMSOLCPIEvent::OrderUpdated(OrderUpdated {
            is_create,
            id,
            ts: 0,
            slot: 0,
            store: STORE,
            order: ORDER,
            market_token: Pubkey::default(),
            owner: Pubkey::default(),
            params,
        })
    }

    /// Feeds transactions (newest first) and finishes as if history ended.
    fn scan(expected_id: Option<u64>, txs: &[Vec<GMSOLCPIEvent>]) -> OrderCompletion {
        let mut scan = LifecycleScan::new(ORDER, expected_id);
        for events in txs {
            if scan.push_transaction(events) {
                return scan.finish(false);
            }
        }
        scan.finish(true)
    }

    fn reason(completion: &OrderCompletion) -> Option<InconclusiveReason> {
        match completion {
            OrderCompletion::Inconclusive { reason, .. } => Some(reason.clone()),
            _ => None,
        }
    }

    fn trade_id(completion: &OrderCompletion) -> Option<u64> {
        match completion {
            OrderCompletion::Trade(trade) => Some(trade.trade_id),
            _ => None,
        }
    }

    #[test]
    fn finds_trade_in_earlier_transaction_than_close() {
        let txs = [
            vec![removed(7)],
            vec![],
            vec![trade(ORDER, 42)],
            vec![created(7)],
        ];
        assert_eq!(trade_id(&scan(Some(7), &txs)), Some(42));
    }

    #[test]
    fn finds_trade_in_same_transaction_as_close() {
        let txs = [vec![trade(ORDER, 42), removed(7)], vec![created(7)]];
        assert_eq!(trade_id(&scan(None, &txs)), Some(42));
    }

    #[test]
    fn ignores_trade_of_another_order() {
        let txs = [
            vec![removed(7)],
            vec![trade(OTHER, 1), trade(ORDER, 42)],
            vec![created(7)],
        ];
        assert_eq!(trade_id(&scan(None, &txs)), Some(42));
    }

    #[test]
    fn stops_at_previous_lifecycle_of_reused_address() {
        // Lifecycle 8 was cancelled; lifecycle 7 at the same address traded.
        let cancelled = GMSOLCPIEvent::OrderRemoved(removed_event(
            8,
            OrderKind::MarketIncrease,
            ActionState::Cancelled,
        ));
        let txs = [vec![cancelled], vec![trade(ORDER, 1), removed(7)]];
        assert!(matches!(scan(None, &txs), OrderCompletion::NoTrade(r) if r.id == 8));
    }

    #[test]
    fn swap_without_trade_is_no_trade() {
        let swap = GMSOLCPIEvent::OrderRemoved(removed_event(
            7,
            OrderKind::MarketSwap,
            ActionState::Completed,
        ));
        assert!(matches!(
            scan(None, &[vec![swap]]),
            OrderCompletion::NoTrade(_)
        ));
    }

    #[test]
    fn completed_position_without_trade_is_inconclusive() {
        let txs = [vec![removed(7)], vec![created(7)]];
        assert_eq!(
            reason(&scan(None, &txs)),
            Some(InconclusiveReason::TradeNotFound)
        );
    }

    #[test]
    fn close_of_unexpected_action_is_inconclusive() {
        let txs = [vec![removed(9)], vec![trade(ORDER, 1)]];
        let completion = scan(Some(7), &txs);
        assert!(matches!(
            completion,
            OrderCompletion::Inconclusive { removed: None, .. }
        ));
        assert_eq!(
            reason(&completion),
            Some(InconclusiveReason::OtherOrderClosed {
                found_id: 9,
                expected_id: 7
            })
        );
    }

    #[test]
    fn scan_limit_without_start_is_inconclusive() {
        let mut txs = vec![vec![removed(7)]];
        txs.extend((0..MAX_SCANNED_TXS).map(|_| vec![]));
        assert_eq!(
            reason(&scan(None, &txs)),
            Some(InconclusiveReason::ScanLimitReached)
        );
    }

    #[test]
    fn missing_close_is_inconclusive() {
        let txs = [vec![trade(ORDER, 42)], vec![created(7)]];
        assert_eq!(
            reason(&scan(None, &txs)),
            Some(InconclusiveReason::CloseNotFound)
        );
    }

    #[test]
    fn stops_at_first_trade_before_reaching_start() {
        // Limit order edited 40 times: the start is further back than the scan limit.
        let mut txs = vec![vec![removed(7)], vec![trade(ORDER, 42)]];
        txs.extend((0..40).map(|_| vec![updated(7, false)]));
        txs.push(vec![created(7)]);
        assert_eq!(trade_id(&scan(Some(7), &txs)), Some(42));
    }

    #[test]
    fn read_error_after_close_keeps_the_close() {
        let mut scan = LifecycleScan::new(ORDER, Some(7));
        assert!(!scan.push_transaction(&[removed(7)]));
        assert!(scan.has_close());
        assert!(matches!(
            scan.finish_with_read_error(&crate::Error::custom("timeout")),
            OrderCompletion::Inconclusive { removed: Some(r), .. } if r.id == 7
        ));
    }

    #[test]
    fn only_missing_events_are_retryable() {
        assert!(InconclusiveReason::CloseNotFound.is_retryable());
        assert!(InconclusiveReason::TradeNotFound.is_retryable());
        assert!(!InconclusiveReason::ScanLimitReached.is_retryable());
        assert!(InconclusiveReason::RpcFailed("x".to_owned()).is_retryable());
        assert!(!InconclusiveReason::DecodeFailed("x".to_owned()).is_retryable());
        assert!(!InconclusiveReason::OtherOrderClosed {
            found_id: 9,
            expected_id: 7
        }
        .is_retryable());
    }

    #[test]
    fn read_errors_split_into_rpc_and_decode() {
        let decode_err = crate::Error::Decode(gmsol_decode::DecodeError::custom("unknown event"));
        let rpc_err = crate::Error::custom("429 Too Many Requests");
        for (err, retryable) in [(decode_err, false), (rpc_err, true)] {
            let mut scan = LifecycleScan::new(ORDER, Some(7));
            scan.push_transaction(&[removed(7)]);
            let reason = reason(&scan.finish_with_read_error(&err)).unwrap();
            assert_eq!(reason.is_retryable(), retryable, "{reason}");
        }
    }

    #[test]
    fn unseen_order_without_close_is_not_found() {
        let no_close = || OrderCompletion::Inconclusive {
            removed: None,
            reason: InconclusiveReason::CloseNotFound,
        };
        assert!(reject_unseen_order(&ORDER, false, no_close()).is_err());
        // Seen open, so it was closed and the close is just not indexed yet.
        assert!(reject_unseen_order(&ORDER, true, no_close()).is_ok());
        let cancelled = OrderCompletion::NoTrade(removed_event(
            7,
            OrderKind::MarketIncrease,
            ActionState::Cancelled,
        ));
        assert!(reject_unseen_order(&ORDER, false, cancelled).is_ok());
    }

    #[test]
    fn same_close_seen_live_and_in_history_is_not_a_boundary() {
        // The live path seeds the close, then history returns the same close again.
        let txs = [
            vec![removed(7)],
            vec![removed(7)],
            vec![trade(ORDER, 42)],
            vec![created(7)],
        ];
        assert_eq!(trade_id(&scan(Some(7), &txs)), Some(42));
    }

    #[test]
    fn edit_then_execute_keeps_the_trade() {
        // Seen open as id 7, edited twice (ids 8, 9), then filled and closed as id 9.
        let txs = [
            vec![removed(9)],
            vec![trade(ORDER, 42)],
            vec![updated(9, false)],
            vec![updated(8, false)],
            vec![created(7)],
        ];
        assert_eq!(trade_id(&scan(Some(7), &txs)), Some(42));
    }

    #[test]
    fn edit_carrying_the_expected_id_confirms_the_order() {
        // Seen open as id 8: the edit that set id 8 is enough, no need to reach the create.
        let mut scan = LifecycleScan::new(ORDER, Some(8));
        assert!(!scan.push_transaction(&[removed(9)]));
        assert!(!scan.push_transaction(&[trade(ORDER, 42)]));
        assert!(!scan.push_transaction(&[updated(9, false)]));
        assert!(scan.push_transaction(&[updated(8, false)]));
        assert_eq!(trade_id(&scan.finish(false)), Some(42));
    }

    #[test]
    fn edited_then_cancelled_order_is_no_trade() {
        let cancelled = GMSOLCPIEvent::OrderRemoved(removed_event(
            9,
            OrderKind::MarketIncrease,
            ActionState::Cancelled,
        ));
        let txs = [
            vec![cancelled],
            vec![updated(9, false)],
            vec![updated(8, false)],
            vec![created(7)],
        ];
        assert!(matches!(scan(Some(7), &txs), OrderCompletion::NoTrade(r) if r.id == 9));
    }

    #[test]
    fn trade_of_another_order_at_the_same_address_is_not_returned() {
        // We saw order id 9 open, but the newest close here is an older order's (id 7).
        let txs = [vec![removed(7)], vec![trade(ORDER, 1)], vec![created(7)]];
        let completion = scan(Some(9), &txs);
        assert!(matches!(
            completion,
            OrderCompletion::Inconclusive { removed: None, .. }
        ));
        assert_eq!(
            reason(&completion),
            Some(InconclusiveReason::OtherOrderClosed {
                found_id: 7,
                expected_id: 9
            })
        );
    }

    #[test]
    fn a_different_close_with_the_same_id_is_a_boundary() {
        // Same id, different slot: an older order at this address, not the same close twice.
        let mut older = removed_event(7, OrderKind::MarketIncrease, ActionState::Completed);
        older.slot = 1;
        let txs = [
            vec![removed(7)],
            vec![GMSOLCPIEvent::OrderRemoved(older)],
            vec![trade(ORDER, 1)],
        ];
        assert_eq!(
            reason(&scan(Some(7), &txs)),
            Some(InconclusiveReason::TradeNotFound)
        );
    }
}

/// Runs [`Client::complete_order`] against real orders that are already closed, so it answers
/// from their on-chain history.
///
/// Ignored by default so CI never depends on a live network. Run them with
/// `cargo test -p gmsol-sdk --features client,decode --lib rpc_tests -- --ignored`, after setting
/// `GMSOL_DEVNET_RPC_URL` and/or `GMSOL_MAINNET_RPC_URL` (e.g. a Helius `https://...?api-key=...`
/// URL; the websocket URL is derived from it). A test whose variable is unset is skipped.
///
/// The orders are pinned. If an owner reuses one of these order addresses, `complete_order`
/// answers for the newest order there, and the pinned values must be updated.
///
/// [`Client::complete_order`]: crate::Client::complete_order
#[cfg(test)]
mod rpc_tests {
    use std::sync::Arc;

    use solana_sdk::{
        commitment_config::CommitmentConfig,
        signature::{Keypair, Signer},
    };

    use super::*;
    use crate::Client;

    const DEVNET: &str = "GMSOL_DEVNET_RPC_URL";
    const MAINNET: &str = "GMSOL_MAINNET_RPC_URL";

    /// `None` when `env` is unset.
    async fn complete(env: &str, order: &str) -> Option<OrderCompletion> {
        let Ok(url) = std::env::var(env) else {
            eprintln!("skipped: {env} is not set");
            return None;
        };
        let client = Client::new(url.parse().unwrap(), Arc::new(Keypair::new())).unwrap();
        let order = order.parse().unwrap();
        let completion = client
            .complete_order(&order, Some(CommitmentConfig::confirmed()))
            .await
            .unwrap();
        Some(completion)
    }

    /// Returns `(trade slot, trade id)`; panics on anything but `Trade` for `order`.
    fn expect_trade(completion: OrderCompletion, order: &str) -> (u64, u64) {
        let OrderCompletion::Trade(trade) = completion else {
            panic!("expected a trade, got {completion:?}");
        };
        assert_eq!(trade.order.to_string(), order);
        (trade.slot, trade.trade_id)
    }

    fn expect_cancelled(completion: OrderCompletion, order: &str) {
        let OrderCompletion::NoTrade(removed) = completion else {
            panic!("expected no trade, got {completion:?}");
        };
        assert_eq!(removed.order.to_string(), order);
        assert!(matches!(removed.state, ActionState::Cancelled));
    }

    /// Traded at slot 513387144, closed at 513387156 in a later transaction. Before this
    /// change `complete_order` read only the close and returned no trade.
    #[tokio::test]
    #[ignore = "needs an RPC node; run with --ignored"]
    async fn devnet_trade_closed_in_a_later_transaction() {
        const ORDER: &str = "HNatsufQwy7xHc7CQsvMvGcVuHCUAwuEUr5tpEmJLHXh";
        let Some(completion) = complete(DEVNET, ORDER).await else {
            return;
        };
        assert_eq!(expect_trade(completion, ORDER), (513_387_144, 151_817));
    }

    /// No order ever used this address: after the default history wait, an error, not
    /// `Inconclusive` ("closed, result unknown").
    #[tokio::test]
    #[ignore = "needs an RPC node; run with --ignored"]
    async fn devnet_unknown_order_is_not_found() {
        let Ok(url) = std::env::var(DEVNET) else {
            eprintln!("skipped: {DEVNET} is not set");
            return;
        };
        let client = Client::new(url.parse().unwrap(), Arc::new(Keypair::new())).unwrap();
        // Random, so no account exists there (`Pubkey::new_unique` gives low addresses that do).
        let order = Keypair::new().pubkey();
        let started = std::time::Instant::now();
        let err = client
            .complete_order(&order, Some(CommitmentConfig::confirmed()))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        assert!(started.elapsed() >= DEFAULT_HISTORY_WAIT);
    }

    #[tokio::test]
    #[ignore = "needs an RPC node; run with --ignored"]
    async fn devnet_cancelled_order() {
        const ORDER: &str = "8uSkNC8Qc793AqGUq5A2UmvF6C18crEYaqE2PvQxAyoQ";
        let Some(completion) = complete(DEVNET, ORDER).await else {
            return;
        };
        expect_cancelled(completion, ORDER);
    }

    /// Executed and closed in the same transaction, at slot 459743651.
    #[tokio::test]
    #[ignore = "needs an RPC node; run with --ignored"]
    async fn mainnet_trade_closed_in_the_same_transaction() {
        const ORDER: &str = "7D5BgckJ8ZwRomaXyjMF14kJMDJTip4dJ5kpfcq4GoAt";
        let Some(completion) = complete(MAINNET, ORDER).await else {
            return;
        };
        assert_eq!(expect_trade(completion, ORDER), (459_743_651, 596_988));
    }

    #[tokio::test]
    #[ignore = "needs an RPC node; run with --ignored"]
    async fn mainnet_cancelled_order() {
        const ORDER: &str = "Dz7adr2ATTh5bmBztcENT3bPhXN6T4moWzF5oPYZCwc7";
        let Some(completion) = complete(MAINNET, ORDER).await else {
            return;
        };
        expect_cancelled(completion, ORDER);
    }
}
