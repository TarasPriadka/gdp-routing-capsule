use std::time::{Instant, Duration};

#[macro_export]
macro_rules! time_it {
    ($context:expr, $s:expr) => {{
        let timer = std::time::Instant::now();
        let a = $s;
        println!("{}: {:?}", $context, timer.elapsed());
        a
    }};
}