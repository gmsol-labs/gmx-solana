/// Get time window index.
pub fn get_time_window_index(ts: i64, time_window: i64) -> i64 {
    debug_assert!(time_window > 0);
    ts / time_window
}
