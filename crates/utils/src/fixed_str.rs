#[derive(Debug, thiserror::Error)]
pub enum FixedStrError {
    /// Exceed max length limit.
    #[error("exceed max length limit")]
    ExceedMaxLengthLimit,
    /// Invalid format.
    #[error("invalid format")]
    InvalidFormat,
    /// Utf8 Error.
    #[error("utf8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}

/// Fixed size string to bytes.
pub fn fixed_str_to_bytes<const MAX_LEN: usize>(
    name: &str,
) -> Result<[u8; MAX_LEN], FixedStrError> {
    let bytes = name.as_bytes();
    if bytes.len() > MAX_LEN {
        return Err(FixedStrError::ExceedMaxLengthLimit);
    }
    let mut buffer = [0; MAX_LEN];
    buffer[..bytes.len()].copy_from_slice(bytes);
    Ok(buffer)
}

/// Bytes to fixed size string.
pub fn bytes_to_fixed_str<const MAX_LEN: usize>(
    bytes: &[u8; MAX_LEN],
) -> Result<&str, FixedStrError> {
    let Some(end) = bytes.iter().position(|&x| x == 0) else {
        return Err(FixedStrError::InvalidFormat);
    };
    let valid_bytes = &bytes[..end];
    Ok(std::str::from_utf8(valid_bytes)?)
}
