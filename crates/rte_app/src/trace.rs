//! Startup stage timing (`--trace-startup`), relative to process creation.

use std::io::Write;

pub struct Trace {
    enabled: bool,
    marks: Vec<(&'static str, f64)>,
}

impl Trace {
    pub fn new(enabled: bool) -> Trace {
        Trace {
            enabled,
            marks: Vec::new(),
        }
    }

    pub fn mark(&mut self, stage: &'static str) {
        if self.enabled {
            self.marks
                .push((stage, rte_platform::ms_since_process_start()));
        }
    }

    /// Prints `startup.<stage>_ms=<value>` lines to stdout (a pipe when traced).
    pub fn report(&self) {
        if !self.enabled {
            return;
        }
        let mut out = std::io::stdout().lock();
        for (stage, ms) in &self.marks {
            let _ = writeln!(out, "startup.{stage}_ms={ms:.3}");
        }
        let _ = out.flush();
    }
}
