use std::sync;

static IS_TRACE: sync::OnceLock<bool> = sync::OnceLock::new();

pub fn is_trace() -> bool {
    *IS_TRACE.get_or_init(|| {
        std::env::var("HW_TRACE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
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
