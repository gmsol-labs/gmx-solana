#[cfg(feature = "store")]
mod store {
    use gmsol_model::price::{Price, Prices};

    use crate::gmsol_store::types;

    impl<T> From<types::Price<T>> for Price<T> {
        fn from(price: types::Price<T>) -> Self {
            Self {
                min: price.min,
                max: price.max,
            }
        }
    }

    impl<T> From<Price<T>> for types::Price<T> {
        fn from(price: Price<T>) -> Self {
            Self {
                min: price.min,
                max: price.max,
            }
        }
    }

    impl<T> From<types::Prices<T>> for Prices<T> {
        fn from(prices: types::Prices<T>) -> Self {
            let types::Prices {
                index_token_price,
                long_token_price,
                short_token_price,
            } = prices;

            Self {
                index_token_price: index_token_price.into(),
                long_token_price: long_token_price.into(),
                short_token_price: short_token_price.into(),
            }
        }
    }

    impl<T> From<Prices<T>> for types::Prices<T> {
        fn from(prices: Prices<T>) -> Self {
            let Prices {
                index_token_price,
                long_token_price,
                short_token_price,
            } = prices;

            Self {
                index_token_price: index_token_price.into(),
                long_token_price: long_token_price.into(),
                short_token_price: short_token_price.into(),
            }
        }
    }
}
