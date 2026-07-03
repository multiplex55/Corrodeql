//! Logging initialization and configuration.

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;

/// Initializes clean command-line logging.
///
/// Normal CLI output defaults to warnings and errors only. Passing `verbose`
/// enables informational diagnostics that are useful while troubleshooting.
pub fn init(verbose: bool) {
    let level = if verbose {
        LevelFilter::INFO
    } else {
        LevelFilter::WARN
    };

    let _ = fmt()
        .with_max_level(level)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .without_time()
        .compact()
        .try_init();
}
