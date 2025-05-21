#[macro_export]
macro_rules! sync_info {
    ($($arg:tt)*) => {
        tracing::info!(target: "sync_log", $($arg)*);
    };
}

#[macro_export]
macro_rules! sync_debug{
    ($($arg:tt)*) => {
        tracing::debug!(target: "sync_log", $($arg)*);
    };
}

#[macro_export]
macro_rules! sync_trace {
    ($($arg:tt)*) => {
        tracing::trace!(target: "sync_log", $($arg)*);
    };
}

#[macro_export]
macro_rules! sync_error{
    ($($arg:tt)*) => {
        tracing::error!(target: "sync_log", $($arg)*);
    }
}
