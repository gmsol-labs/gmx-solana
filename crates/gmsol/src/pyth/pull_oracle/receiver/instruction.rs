use anchor_client::anchor_lang::{
    prelude::{borsh, AnchorSerialize},
    Discriminator, InstructionData,
};
use pythnet_sdk::wire::v1::MerklePriceUpdate;

#[derive(AnchorSerialize)]
pub(super) struct PostUpdate {
    pub(super) merkle_price_update: MerklePriceUpdate,
    pub(super) treasury_id: u8,
}

impl Discriminator for PostUpdate {
    const DISCRIMINATOR: &'static [u8] = &[133, 95, 207, 175, 11, 79, 118, 44];
}

impl InstructionData for PostUpdate {}

#[derive(AnchorSerialize)]
pub(super) struct ReclaimRent {}

impl Discriminator for ReclaimRent {
    const DISCRIMINATOR: &'static [u8] = &[218, 200, 19, 197, 227, 89, 192, 22];
}

impl InstructionData for ReclaimRent {}
