use std::marker::PhantomData;

use solana_sdk::signature::Signature;

use crate::{Decode, Visitor};

use super::OwnedData;

/// Anchor CPI Events.
#[derive(Debug, Clone)]
pub struct AnchorCPIEvents<T> {
    slot: u64,
    index: Option<usize>,
    signature: Signature,
    events: Vec<OwnedData<T>>,
}

impl<T> AnchorCPIEvents<T> {
    /// Get the slot at which the events were generated.
    pub fn slot(&self) -> u64 {
        self.slot
    }

    /// Get the `index` in the block of the transaction where the events were genereated.
    pub fn index(&self) -> Option<usize> {
        self.index
    }

    /// Get the `signature` of the transaction where the events were generated.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    /// Get the event datas.
    pub fn events(&self) -> &[OwnedData<T>] {
        &self.events
    }
}

impl<T> IntoIterator for AnchorCPIEvents<T> {
    type Item = OwnedData<T>;

    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

impl<T> Decode for AnchorCPIEvents<T>
where
    T: Decode,
{
    fn decode<D: crate::Decoder>(decoder: D) -> Result<Self, crate::DecodeError> {
        struct CPIEventsData<T>(PhantomData<T>);

        impl<T> Visitor for CPIEventsData<T>
        where
            T: Decode,
        {
            type Value = AnchorCPIEvents<T>;

            fn visit_anchor_cpi_events<'a>(
                self,
                mut events: impl crate::AnchorCPIEventsAccess<'a>,
            ) -> Result<Self::Value, crate::DecodeError> {
                let slot = events.slot()?;
                let index = events.index()?;
                let signature = *events.signature()?;
                let events =
                    std::iter::repeat_with(|| events.next_event::<OwnedData<T>>().transpose())
                        .take_while(Option::is_some)
                        .flatten()
                        .collect::<Result<Vec<_>, crate::DecodeError>>()?;
                Ok(AnchorCPIEvents {
                    signature,
                    slot,
                    index,
                    events,
                })
            }
        }

        let events = decoder.decode_anchor_cpi_events(CPIEventsData::<T>(PhantomData))?;

        Ok(events)
    }
}
