use std::time::{Instant, Duration};

#[macro_export]
macro_rules! time_it {
    ($machine:expr, $message:expr, $s:expr) => {{
        let timer = std::time::Instant::now();
        let a = $s;
        println!("TIMING|{}|{}|{:?}", $machine, $message, timer.elapsed().as_micros());
        a
    }};
}
