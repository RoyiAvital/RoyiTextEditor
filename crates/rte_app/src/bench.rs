//! Scripted input for interaction benchmarks (`--bench-keys`, `--bench-runs`).
//! Once the file is loaded and shown, the keys are sent one at a time; each is
//! timed from its handling to the presented frame (after `BitBlt`). The times
//! are printed as `bench.<step>_ms=<value>` lines, then the editor closes.

use std::io::Write;
use std::time::Instant;

use rte_platform::{Ctx, Key};

use crate::commands::parse_key;

pub struct Script {
    pub keys: Vec<Key>,
    pub runs: usize,
}

/// Parses comma-separated keys (`Ctrl+Shift+P,Escape`) repeated `runs` times.
pub fn parse(keys: &str, runs: &str) -> Result<Script, String> {
    let keys = keys
        .split(',')
        .map(|k| parse_key(k).ok_or_else(|| format!("unknown key {k:?}")))
        .collect::<Result<Vec<_>, _>>()?;
    let runs = runs
        .parse()
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("invalid run count {runs:?}"))?;
    Ok(Script { keys, runs })
}

pub struct Driver {
    script: Script,
    step: usize,
    run: usize,
    /// A key was posted and not yet presented.
    sent: bool,
    started: Option<Instant>,
    /// Milliseconds per step, in run order.
    times: Vec<Vec<f64>>,
}

impl Driver {
    pub fn new(script: Script) -> Driver {
        let steps = script.keys.len();
        Driver {
            script,
            step: 0,
            run: 0,
            sent: false,
            started: None,
            times: vec![Vec::new(); steps],
        }
    }

    /// Posts the next key unless one is in flight.
    pub fn send(&mut self, ctx: &mut Ctx) {
        if !self.sent && self.run < self.script.runs {
            ctx.post_key(self.script.keys[self.step]);
            self.sent = true;
        }
    }

    /// Starts timing when `key` is the posted one.
    pub fn key_arrived(&mut self, key: Key) {
        if self.sent && self.started.is_none() && key == self.script.keys[self.step] {
            self.started = Some(Instant::now());
        }
    }

    /// The posted key was handled without a visible change: it cannot be timed.
    pub fn check_frame(&mut self, redraw: bool, ctx: &mut Ctx) {
        if self.started.is_some() && !redraw {
            println!("bench.step{}_no_frame", self.step);
            ctx.close();
        }
    }

    pub fn presented(&mut self, ctx: &mut Ctx) {
        let Some(started) = self.started.take() else {
            return;
        };
        self.times[self.step].push(started.elapsed().as_secs_f64() * 1000.0);
        self.sent = false;
        self.step = (self.step + 1) % self.script.keys.len();
        if self.step == 0 {
            self.run += 1;
        }
        if self.run == self.script.runs {
            self.report();
            ctx.close();
        } else {
            self.send(ctx);
        }
    }

    fn report(&self) {
        let mut out = std::io::stdout().lock();
        for run in 0..self.script.runs {
            for (step, times) in self.times.iter().enumerate() {
                let _ = writeln!(out, "bench.{step}_ms={:.4}", times[run]);
            }
        }
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse_keys_and_runs() {
        let script = parse("Ctrl+Shift+P, Escape", "3").unwrap();
        assert_eq!(script.runs, 3);
        assert_eq!(script.keys.len(), 2);
        assert!(script.keys[0].ctrl && script.keys[0].shift);
        assert!(parse("Ctrl+Bogus", "1").is_err());
        assert!(parse("Escape", "0").is_err());
    }

    #[test]
    fn the_driver_times_each_key_and_closes_after_the_last_run() {
        let script = parse("Ctrl+P,Escape", "2").unwrap();
        let keys = script.keys.clone();
        let mut driver = Driver::new(script);
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.send(&mut ctx);
        let mut posted = ctx.posted_key();
        for _ in 0..4 {
            let key = posted.unwrap();
            driver.key_arrived(key);
            let mut ctx = Ctx::headless(10, 10, 96);
            driver.check_frame(true, &mut ctx);
            driver.presented(&mut ctx);
            posted = ctx.posted_key();
            if posted.is_none() {
                assert!(ctx.close_requested());
            }
        }
        assert_eq!(driver.times[0].len(), 2);
        assert_eq!(driver.times[1].len(), 2);
        assert_eq!(keys[1].vk, 0x1B);
    }
}
