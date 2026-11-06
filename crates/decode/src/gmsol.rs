/// Implement [`Decode`](crate::Decode) for types defined in [`gmsol-programs`].
#[cfg(feature = "gmsol-programs")]
pub mod programs {
    use crate::{
        impl_decode_for_account_deserialize, impl_decode_for_cpi_event, impl_decode_for_zero_copy,
    };
    use crate::{untagged, value::UnknownOwnedData};
    use gmsol_programs::gmsol_evm_proxy::accounts::Config;
    use gmsol_programs::gmsol_store::{
        accounts::{
            Deposit, Glv, GlvDeposit, GlvShift, GlvWithdrawal, Market, Order, Position, Shift,
            Store, UserHeader, VirtualInventory, Withdrawal,
        },
        events::{
            BorrowingFeesUpdated, BuilderFeeCharged, BuilderFeeClaimed, BuilderFeeFactorSet,
            BuilderFeeSet, BuilderFeeSettled, DepositExecuted, DepositRemoved, GlvDepositRemoved,
            GlvPricing, GlvTokenValue, GlvWithdrawalRemoved, GtAllowanceConsumed, GtBuyback,
            GtDelegationUpdated, GtUpdated, InsufficientFundingFeePayment, MarketFeesUpdated,
            MarketStateUpdated, MarketTokenValue, OrderRemoved, OrderUpdated, PositionDecreased,
            PositionIncreased, ShiftRemoved, SwapExecuted, TradeEvent, WithdrawalExecuted,
            WithdrawalRemoved,
        },
    };

    impl_decode_for_zero_copy!(Store);
    impl_decode_for_account_deserialize!(Config);
    impl_decode_for_zero_copy!(Position);
    impl_decode_for_zero_copy!(Market);
    impl_decode_for_zero_copy!(VirtualInventory);
    impl_decode_for_zero_copy!(Deposit);
    impl_decode_for_zero_copy!(Withdrawal);
    impl_decode_for_zero_copy!(Shift);
    impl_decode_for_zero_copy!(Order);
    impl_decode_for_zero_copy!(GlvDeposit);
    impl_decode_for_zero_copy!(GlvWithdrawal);
    impl_decode_for_zero_copy!(GlvShift);
    impl_decode_for_zero_copy!(Glv);
    impl_decode_for_zero_copy!(UserHeader);

    impl_decode_for_cpi_event!(DepositRemoved);
    impl_decode_for_cpi_event!(DepositExecuted);
    impl_decode_for_cpi_event!(WithdrawalRemoved);
    impl_decode_for_cpi_event!(WithdrawalExecuted);
    impl_decode_for_cpi_event!(ShiftRemoved);
    impl_decode_for_cpi_event!(GlvDepositRemoved);
    impl_decode_for_cpi_event!(GlvWithdrawalRemoved);
    impl_decode_for_cpi_event!(GlvPricing);
    impl_decode_for_cpi_event!(OrderUpdated);
    impl_decode_for_cpi_event!(PositionIncreased);
    impl_decode_for_cpi_event!(PositionDecreased);
    impl_decode_for_cpi_event!(InsufficientFundingFeePayment);
    impl_decode_for_cpi_event!(OrderRemoved);
    impl_decode_for_cpi_event!(TradeEvent);
    impl_decode_for_cpi_event!(MarketFeesUpdated);
    impl_decode_for_cpi_event!(BorrowingFeesUpdated);
    impl_decode_for_cpi_event!(MarketStateUpdated);
    impl_decode_for_cpi_event!(SwapExecuted);
    impl_decode_for_cpi_event!(GtUpdated);
    impl_decode_for_cpi_event!(GtBuyback);
    impl_decode_for_cpi_event!(MarketTokenValue);
    impl_decode_for_cpi_event!(GlvTokenValue);
    impl_decode_for_cpi_event!(BuilderFeeCharged);
    impl_decode_for_cpi_event!(BuilderFeeFactorSet);
    impl_decode_for_cpi_event!(GtDelegationUpdated);
    impl_decode_for_cpi_event!(GtAllowanceConsumed);
    impl_decode_for_cpi_event!(BuilderFeeSet);
    impl_decode_for_cpi_event!(BuilderFeeSettled);
    impl_decode_for_cpi_event!(BuilderFeeClaimed);

    untagged!(
        GMSOLAccountData,
        [
            Deposit,
            Withdrawal,
            Shift,
            Order,
            GlvDeposit,
            GlvWithdrawal,
            GlvShift,
            Glv,
            Store,
            Market,
            VirtualInventory,
            Position,
            UserHeader,
            Config,
            UnknownOwnedData
        ]
    );

    type Account = crate::value::Account<GMSOLAccountData>;

    untagged!(
        GMSOLCPIEvent,
        [
            DepositExecuted,
            DepositRemoved,
            WithdrawalExecuted,
            WithdrawalRemoved,
            ShiftRemoved,
            GlvDepositRemoved,
            GlvWithdrawalRemoved,
            GlvPricing,
            OrderUpdated,
            PositionIncreased,
            PositionDecreased,
            InsufficientFundingFeePayment,
            OrderRemoved,
            TradeEvent,
            MarketFeesUpdated,
            BorrowingFeesUpdated,
            MarketStateUpdated,
            SwapExecuted,
            GtUpdated,
            GtBuyback,
            MarketTokenValue,
            GlvTokenValue,
            BuilderFeeCharged,
            BuilderFeeFactorSet,
            GtDelegationUpdated,
            GtAllowanceConsumed,
            BuilderFeeSet,
            BuilderFeeSettled,
            BuilderFeeClaimed,
            UnknownOwnedData
        ]
    );

    type CPIEvents = crate::value::AnchorCPIEvents<GMSOLCPIEvent>;

    untagged!(GMSOLData, [Account, CPIEvents]);
}

/// Decoding for EVM proxy Config logs, which use Anchor `emit!` rather than CPI events.
#[cfg(feature = "gmsol-programs")]
pub mod evm_proxy {
    use anchor_lang::{AnchorDeserialize, Discriminator};
    use base64::Engine;
    use gmsol_programs::gmsol_evm_proxy::events;

    /// One decoded Config change event.
    #[derive(Debug)]
    pub enum ConfigEvent {
        /// Initial Config state.
        Created(events::ConfigCreated),
        /// Chain policy change.
        ChainAllowedChanged(events::ConfigChainAllowedChanged),
        /// Pause state change.
        PauseChanged(events::ConfigPauseChanged),
        /// Pending administrator proposal or replacement.
        AdminProposed(events::ConfigAdminProposed),
        /// Administrator acceptance.
        AdminAccepted(events::ConfigAdminAccepted),
    }

    /// Decode a proxy `Program data:` log. Callers must first attribute it to this
    /// program and require a successful transaction before applying its change.
    pub fn decode_config_event_log(line: &str) -> Result<Option<ConfigEvent>, crate::DecodeError> {
        let Some(encoded) = line.strip_prefix("Program data: ") else {
            return Ok(None);
        };
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(crate::DecodeError::custom)?;
        macro_rules! decode {
            ($event:ty, $variant:ident) => {
                if data.starts_with(<$event>::DISCRIMINATOR) {
                    let event = <$event>::deserialize(&mut &data[<$event>::DISCRIMINATOR.len()..])
                        .map_err(crate::DecodeError::custom)?;
                    return Ok(Some(ConfigEvent::$variant(event)));
                }
            };
        }
        decode!(events::ConfigCreated, Created);
        decode!(events::ConfigChainAllowedChanged, ChainAllowedChanged);
        decode!(events::ConfigPauseChanged, PauseChanged);
        decode!(events::ConfigAdminProposed, AdminProposed);
        decode!(events::ConfigAdminAccepted, AdminAccepted);
        Ok(None)
    }

    /// Decode a consumed intent from a successful EVM proxy transaction's log.
    pub fn decode_intent_event_log(
        line: &str,
    ) -> Result<Option<events::IntentConsumed>, crate::DecodeError> {
        let Some(encoded) = line.strip_prefix("Program data: ") else {
            return Ok(None);
        };
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(crate::DecodeError::custom)?;
        if !data.starts_with(events::IntentConsumed::DISCRIMINATOR) {
            return Ok(None);
        }
        events::IntentConsumed::deserialize(
            &mut &data[events::IntentConsumed::DISCRIMINATOR.len()..],
        )
        .map(Some)
        .map_err(crate::DecodeError::custom)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{gmsol::programs::GMSOLAccountData, value::OwnedDataDecoder, Decode};
        use anchor_lang::{AnchorSerialize, Event};
        use gmsol_programs::gmsol_evm_proxy::{accounts::Config, ID};
        use solana_sdk::pubkey::Pubkey;

        #[test]
        fn decodes_proxy_config_account() {
            let config = Config {
                store: Pubkey::new_unique(),
                genesis_hash: [7; 32],
                admin: Pubkey::new_unique(),
                pending_admin: None,
                paused: true,
                allowed_chain_ids: vec![],
            };
            let mut data = Config::DISCRIMINATOR.to_vec();
            config.serialize(&mut data).unwrap();
            let decoded = GMSOLAccountData::decode(OwnedDataDecoder::new(&ID, &data)).unwrap();
            let GMSOLAccountData::Config(decoded) = decoded else {
                panic!("expected Config account");
            };
            assert_eq!(decoded.store, config.store);
            assert_eq!(decoded.genesis_hash, config.genesis_hash);
        }

        #[test]
        fn decodes_proxy_event_log() {
            let event = events::ConfigPauseChanged {
                config: Pubkey::new_unique(),
                previous_paused: true,
                paused: false,
            };
            let line = format!(
                "Program data: {}",
                base64::engine::general_purpose::STANDARD.encode(event.data())
            );
            let Some(ConfigEvent::PauseChanged(decoded)) = decode_config_event_log(&line).unwrap()
            else {
                panic!("expected pause event");
            };
            assert_eq!(decoded.config, event.config);
            assert!(decoded.previous_paused);
            assert!(!decoded.paused);
        }

        #[test]
        fn decodes_consumed_pair_event() {
            let event = events::IntentConsumed {
                proxy: Pubkey::new_unique(),
                generation: 8,
                nonce: 0,
                kind: 1,
            };
            let line = format!(
                "Program data: {}",
                base64::engine::general_purpose::STANDARD.encode(event.data())
            );
            let decoded = decode_intent_event_log(&line).unwrap().unwrap();
            assert_eq!(decoded.proxy, event.proxy);
            assert_eq!(decoded.generation, event.generation);
            assert_eq!(decoded.nonce, event.nonce);
            assert_eq!(decoded.kind, event.kind);
            assert!(decode_config_event_log(&line).unwrap().is_none());
        }
    }
}
