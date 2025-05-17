/// Fixed size string to bytes.
pub fn fixed_str_to_bytes<const MAX_LEN: usize>(name: &str) -> crate::Result<[u8; MAX_LEN]> {
    let bytes = name.as_bytes();
    if bytes.len() > MAX_LEN {
        return Err(crate::Error::custom("exceed max length limit"));
    }
    let mut buffer = [0; MAX_LEN];
    buffer[..bytes.len()].copy_from_slice(bytes);
    Ok(buffer)
}

/// Bytes to fixed size string.
pub fn bytes_to_fixed_str<const MAX_LEN: usize>(bytes: &[u8; MAX_LEN]) -> crate::Result<&str> {
    let Some(end) = bytes.iter().position(|&x| x == 0) else {
        return Err(crate::Error::custom("invalid str"));
    };
    let valid_bytes = &bytes[..end];
    std::str::from_utf8(valid_bytes).map_err(|_| crate::Error::custom("invalid str"))
}
