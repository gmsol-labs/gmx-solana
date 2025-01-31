/// Implement [`Decode`](crate::Decode) for a list of types.
#[macro_export]
macro_rules! untagged {
    ($name:ident, [$($decoded:ty),+]) => {
        $crate::paste::paste! {
            #[doc = "A untagged decodable enum `" $name "`."]
            #[allow(clippy::large_enum_variant)]
            #[derive(Debug)]
            pub enum $name {
                $(
                    #[doc = "Variant `" $decoded "`."]
                    [<$decoded>]($decoded)
                ),+
            }

            impl $crate::Decode for $name {
                #[allow(unused_variables)]
                fn decode<D: $crate::Decoder>(decoder: D) -> Result<Self, $crate::DecodeError> {
                    $(
                        match $decoded::decode(&decoder) {
                            Ok(decoded) => {
                                return Ok($name::[<$decoded>](decoded));
                            },
                            Err(err) => {
                                $crate::tracing::trace!(%err, "try variant `{}::{}`, failed", stringify!($name), stringify!($decoded));
                            }
                        }
                    );+
                    Err($crate::DecodeError::custom("no matching variant found"))
                }
            }
        }
    };
}
