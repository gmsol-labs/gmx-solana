use std::mem::size_of;

use bytemuck::Pod;

/// Get at index.
pub fn get<T: Pod>(data: &[u8], index: usize) -> Option<&T> {
    let start = index.checked_mul(size_of::<T>())?;
    let end = start.checked_add(size_of::<T>())?;
    if data.len() < end {
        None
    } else {
        Some(bytemuck::from_bytes(&data[start..end]))
    }
}

/// Get mutablely at index.
pub fn get_mut<T: Pod>(data: &mut [u8], index: usize) -> Option<&mut T> {
    let start = index.checked_mul(size_of::<T>())?;
    let end = start.checked_add(size_of::<T>())?;
    if data.len() < end {
        None
    } else {
        Some(bytemuck::from_bytes_mut(&mut data[start..end]))
    }
}
