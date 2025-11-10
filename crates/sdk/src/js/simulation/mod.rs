pub mod deposit;
pub mod order;
pub mod simulator;
pub mod withdrawal;

pub use order::{JsOrderSimulationOutput, SimulateOrderArgs};
pub use simulator::JsSimulator;

use borsh::BorshSerialize;
use gmsol_programs::bytemuck;

use crate::utils::base64::encode_base64;

fn encode_borsh_base64<T: BorshSerialize>(data: &T) -> crate::Result<String> {
    data.try_to_vec()
        .map(|data| encode_base64(&data))
        .map_err(crate::Error::custom)
}

fn encode_bytemuck_base64<T: bytemuck::NoUninit>(data: &T) -> String {
    encode_base64(bytemuck::bytes_of(data))
}
