//! Compiles on Oath take half the machine's CPUs: its owner is using the
//! rest. By affinity, not `CARGO_BUILD_JOBS`: `oath build`'s sandbox starts
//! its build with a clean environment, while affinity and niceness pass
//! through `sudo` and the sandbox to every process of the build, and cargo
//! sizes its jobs from the affinity. Only the compile: what `oath apply`
//! starts must keep every CPU.

use std::process::Command;

/// A command that runs `program` on half the CPUs, niced.
pub fn limited(program: &str) -> Command {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut cmd = Command::new("taskset");
    cmd.args(["-c", &half(cpus), "nice", "-n", "10", program]);
    cmd
}

/// The `taskset` list for half of `cpus`.
fn half(cpus: usize) -> String {
    match cpus / 2 {
        0 | 1 => "0".into(),
        n => format!("0-{}", n - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_of_the_cpus_and_at_least_one() {
        assert_eq!(half(32), "0-15");
        assert_eq!(half(8), "0-3");
        assert_eq!(half(3), "0");
        assert_eq!(half(1), "0");
    }
}
