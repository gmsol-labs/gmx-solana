use gmsol_programs::anchor_lang::{
    prelude::{borsh, AnchorSerialize},
    Discriminator, InstructionData,
};

#[derive(AnchorSerialize)]
pub(super) struct InitEncodedVaa {}

impl Discriminator for InitEncodedVaa {
    const DISCRIMINATOR: &[u8] = &[209, 193, 173, 25, 91, 202, 181, 218];
}

impl InstructionData for InitEncodedVaa {}

#[derive(AnchorSerialize)]
pub(super) struct WriteEncodedVaa {
    pub(super) index: u32,
    pub(super) data: Vec<u8>,
}

impl Discriminator for WriteEncodedVaa {
    const DISCRIMINATOR: &[u8] = &[199, 208, 110, 177, 150, 76, 118, 42];
}

impl InstructionData for WriteEncodedVaa {}

#[derive(AnchorSerialize)]
pub(super) struct VerifyEncodedVaaV1 {}

impl Discriminator for VerifyEncodedVaaV1 {
    const DISCRIMINATOR: &[u8] = &[103, 56, 177, 229, 240, 103, 68, 73];
}

impl InstructionData for VerifyEncodedVaaV1 {}

#[derive(AnchorSerialize)]
pub(super) struct CloseEncodedVaa {}

impl Discriminator for CloseEncodedVaa {
    const DISCRIMINATOR: &[u8] = &[48, 221, 174, 198, 231, 7, 152, 38];
}

impl InstructionData for CloseEncodedVaa {}
