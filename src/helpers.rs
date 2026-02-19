use std::sync::OnceLock;

static IS_TRACE: OnceLock<bool> = OnceLock::new();

pub fn is_trace() -> bool {
    *IS_TRACE.get_or_init(|| {
        std::env::var("HW_TRACE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

#[macro_export]
macro_rules! trace {
    ($($stmt:stmt)*) => {
        if $crate::helpers::is_trace() {
            $($stmt)*
        }
    };
}
