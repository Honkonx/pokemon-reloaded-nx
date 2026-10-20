//! Output and allocation macros that `core` does not provide.

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::io::_print(::core::format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! println {
    () => { $crate::io::_print(::core::format_args!("\n")) };
    ($($arg:tt)*) => { $crate::io::_print(::core::format_args!("{}\n", ::core::format_args!($($arg)*))) };
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::io::_eprint(::core::format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! eprintln {
    () => { $crate::io::_eprint(::core::format_args!("\n")) };
    ($($arg:tt)*) => { $crate::io::_eprint(::core::format_args!("{}\n", ::core::format_args!($($arg)*))) };
}

#[macro_export]
macro_rules! format {
    ($($arg:tt)*) => { $crate::__alloc::format!($($arg)*) };
}

#[macro_export]
macro_rules! vec {
    ($($arg:tt)*) => { $crate::__alloc::vec!($($arg)*) };
}
