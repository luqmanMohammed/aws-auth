#[macro_export]
macro_rules! elog {
    ($enabled:expr, $($args:tt)*) => {
        if $enabled {
            eprintln!($($args)*)
        }
    };
}
