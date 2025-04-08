use std::env;

use tracing::subscriber::set_default;
use tracing_subscriber::EnvFilter;

/// Setup fmt tracing subscriber.
pub fn setup_fmt_tracing(default_rust_log: &str) -> impl Drop {
    if env::var(EnvFilter::DEFAULT_ENV).is_err() {
        env::set_var(EnvFilter::DEFAULT_ENV, default_rust_log);
    }
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::builder().from_env_lossy())
        .finish();
    set_default(subscriber)
}
