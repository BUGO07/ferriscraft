use std::collections::VecDeque;

#[macro_export]
macro_rules! log {
    ($logs:expr, $($arg:tt)*) => ($crate::utils::_log($logs, format_args!($($arg)*)));
}

pub fn _log(logs: &mut VecDeque<String>, args: std::fmt::Arguments) {
    let s = chrono::Local::now().format("[%H:%M:%S] ").to_string() + &args.to_string();
    println!("{}", s);
    if logs.len() == 256 {
        logs.pop_front();
    }
    logs.push_back(s);
}
