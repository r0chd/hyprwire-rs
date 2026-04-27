use std::env;
use std::sync::atomic;

/// State for the trace cache: 0 = uncached, 1 = cached false, 2 = cached true.
static TRACE_STATE: atomic::AtomicU8 = atomic::AtomicU8::new(0);

fn read_trace_env() -> bool {
    env::var("HW_TRACE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn is_trace() -> bool {
    loop {
        match TRACE_STATE.load(atomic::Ordering::Relaxed) {
            0 => {
                let val = read_trace_env();
                let encoded: u8 = if val { 2 } else { 1 };
                let _ = TRACE_STATE.compare_exchange(
                    0,
                    encoded,
                    atomic::Ordering::Relaxed,
                    atomic::Ordering::Relaxed,
                );
            }
            1 => return false,
            _ => return true,
        }
    }
}

/// Reset the cached trace value so that the next call to [`is_trace`] re-reads
/// the environment variable. Intended for use in tests.
pub fn reset_trace_cache() {
    TRACE_STATE.store(0, atomic::Ordering::Relaxed);
}

#[allow(missing_docs)]
#[macro_export]
macro_rules! trace {
    ($($tt:tt)*) => {
        if $crate::helpers::is_trace() {
            $($tt)*
        }
    };
}
