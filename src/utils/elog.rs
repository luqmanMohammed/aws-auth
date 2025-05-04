#[macro_export]
macro_rules! elog {
    ($silent:expr, $($args:tt)*) => {
        if !$silent {
            eprintln!($($args)*)
        }
    };
}
