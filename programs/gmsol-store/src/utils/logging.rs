#[macro_export]
macro_rules! debug_msg {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug-msg")]
        msg!($($arg)*)
    };
}
