//! Scripted input for interaction benchmarks (`--bench-keys`, `--bench-runs`,
//! `--bench-setup`). Once the file is loaded and shown, the setup steps are
//! sent untimed, then the steps one at a time; each is timed from its handling
//! to the last presented frame of its work (search results included). The
//! times are printed as `bench.<step>_ms=<value>` lines, followed by
//! `bench.peak_private_mib=` (and `bench.refresh_ms=` when paced), then the
//! editor closes.
//!
//! A script is a comma-separated list of keys (`Ctrl+Shift+P`, `Down*40` for
//! 40 presses) and quoted
//! text (`'abc'`), which is typed one character per step. Options:
//! `--bench-select <start>..<end>` selects bytes before the setup;
//! `--bench-handled` ends a step that draws nothing when it is handled (copies);
//! `--bench-paced` sends a step one display refresh after the previous frame,
//! like a held key.

use std::io::Write;
use std::ops::Range;
use std::time::Instant;

use rte_platform::{Ctx, Key};

use crate::commands::parse_key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Key(Key),
    Char(char),
}

pub struct Script {
    /// Sent once before the timed steps.
    pub setup: Vec<Step>,
    pub keys: Vec<Step>,
    pub runs: usize,
    /// Selected once the file is loaded, before the setup.
    pub select: Option<Range<u64>>,
    pub handled: bool,
    pub paced: bool,
}

/// `<start>..<end>` byte offsets.
pub fn parse_range(text: &str) -> Result<Range<u64>, String> {
    let (start, end) = text
        .split_once("..")
        .ok_or_else(|| format!("expected <start>..<end>, got {text:?}"))?;
    let number = |n: &str| {
        n.trim()
            .parse::<u64>()
            .map_err(|_| format!("invalid offset {n:?}"))
    };
    Ok(number(start)?..number(end)?)
}

fn parse_steps(script: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    let mut rest = script;
    while !rest.trim().is_empty() {
        let item = rest.trim_start();
        let after = match item.strip_prefix('\'') {
            Some(text) => {
                let end = text
                    .find('\'')
                    .ok_or_else(|| format!("unclosed text in {script:?}"))?;
                if end == 0 {
                    return Err("empty text ''".to_owned());
                }
                steps.extend(text[..end].chars().map(Step::Char));
                &text[end + 1..]
            }
            None => {
                let end = item.find(',').unwrap_or(item.len());
                let (key, times) = match item[..end].trim().rsplit_once('*') {
                    Some((key, n)) => {
                        let n = n
                            .trim()
                            .parse::<usize>()
                            .map_err(|_| format!("invalid repeat {n:?}"))?;
                        (key.trim(), n)
                    }
                    None => (item[..end].trim(), 1),
                };
                let key = parse_key(key).ok_or_else(|| format!("unknown key {key:?}"))?;
                steps.extend(std::iter::repeat_n(Step::Key(key), times));
                &item[end..]
            }
        };
        let after = after.trim_start();
        rest = match after.strip_prefix(',') {
            Some(next) => next,
            None if after.is_empty() => after,
            None => return Err(format!("expected a comma before {after:?}")),
        };
    }
    Ok(steps)
}

/// Parses the steps (`Ctrl+Shift+P,Escape`, `'text'`) repeated `runs` times,
/// after the untimed `setup` steps.
pub fn parse(keys: &str, runs: &str, setup: &str) -> Result<Script, String> {
    let keys = parse_steps(keys)?;
    if keys.is_empty() {
        return Err("no keys".to_owned());
    }
    let runs = runs
        .parse()
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("invalid run count {runs:?}"))?;
    Ok(Script {
        setup: parse_steps(setup)?,
        keys,
        runs,
        select: None,
        handled: false,
        paced: false,
    })
}

pub struct Driver {
    script: Script,
    /// Setup keys sent so far.
    setup: usize,
    step: usize,
    run: usize,
    /// A key was posted and not yet presented.
    sent: bool,
    started: Option<Instant>,
    /// A frame was presented since the key while its work was still running.
    shown: bool,
    /// A frame was requested since the key and not presented yet.
    requested: bool,
    /// Milliseconds per step, in run order.
    times: Vec<Vec<f64>>,
}

impl Driver {
    pub fn new(script: Script) -> Driver {
        let steps = script.keys.len();
        Driver {
            script,
            setup: 0,
            step: 0,
            run: 0,
            sent: false,
            started: None,
            shown: false,
            requested: false,
            times: vec![Vec::new(); steps],
        }
    }

    fn in_setup(&self) -> bool {
        self.setup < self.script.setup.len()
    }

    fn current(&self) -> Step {
        if self.in_setup() {
            self.script.setup[self.setup]
        } else {
            self.script.keys[self.step]
        }
    }

    /// Takes the selection to make before the setup, once.
    pub fn take_select(&mut self) -> Option<Range<u64>> {
        self.script.select.take()
    }

    /// Posts the next step unless one is in flight.
    pub fn send(&mut self, ctx: &mut Ctx) {
        if !self.sent && self.run < self.script.runs {
            match self.current() {
                Step::Key(key) => ctx.post_key(key),
                Step::Char(ch) => ctx.post_char(ch),
            }
            self.sent = true;
        }
    }

    /// Starts timing when `step` is the posted one.
    pub fn arrived(&mut self, step: Step) {
        if self.sent && self.started.is_none() && step == self.current() {
            self.started = Some(Instant::now());
            self.shown = false;
            self.requested = false;
        }
    }

    /// After each event: a key whose work ended without a new frame is done
    /// (or, without any frame, cannot be timed). `busy`: work of the key runs.
    pub fn check_frame(&mut self, redraw: bool, busy: bool, ctx: &mut Ctx) {
        if self.started.is_none() {
            return;
        }
        self.requested |= redraw;
        if self.requested || busy {
            return;
        }
        if self.shown || self.in_setup() || self.script.handled {
            self.finish(ctx);
        } else {
            println!("bench.step{}_no_frame", self.step);
            ctx.close();
        }
    }

    pub fn presented(&mut self, busy: bool, ctx: &mut Ctx) {
        if self.started.is_none() {
            return;
        }
        self.requested = false;
        if busy {
            self.shown = true;
            return;
        }
        self.finish(ctx);
    }

    fn finish(&mut self, ctx: &mut Ctx) {
        let Some(started) = self.started.take() else {
            return;
        };
        self.sent = false;
        if self.in_setup() {
            self.setup += 1;
            self.send(ctx);
            return;
        }
        self.times[self.step].push(started.elapsed().as_secs_f64() * 1000.0);
        self.step = (self.step + 1) % self.script.keys.len();
        if self.step == 0 {
            self.run += 1;
        }
        if self.run == self.script.runs {
            self.report();
            ctx.close();
        } else {
            if self.script.paced {
                rte_platform::wait_for_composition();
            }
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
        let peak = rte_platform::peak_private_bytes() as f64 / (1024.0 * 1024.0);
        let _ = writeln!(out, "bench.peak_private_mib={peak:.2}");
        if self.script.paced {
            let _ = writeln!(
                out,
                "bench.refresh_ms={:.3}",
                rte_platform::refresh_period_ms()
            );
        }
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_of(step: Step) -> Key {
        match step {
            Step::Key(key) => key,
            Step::Char(ch) => panic!("{ch:?} is not a key"),
        }
    }

    #[test]
    fn scripts_parse_keys_and_runs() {
        let script = parse("Ctrl+Shift+P, Escape", "3", "").unwrap();
        assert_eq!(script.runs, 3);
        assert_eq!(script.keys.len(), 2);
        assert!(script.setup.is_empty());
        let first = key_of(script.keys[0]);
        assert!(first.ctrl && first.shift);
        assert!(parse("Ctrl+Bogus", "1", "").is_err());
        assert!(parse("Escape", "0", "").is_err());
        assert!(parse("Escape", "1", "Ctrl+Bogus").is_err());
        assert_eq!(parse("F3", "1", "Ctrl+F,Alt+R").unwrap().setup.len(), 2);
    }

    #[test]
    fn repeats_ranges_and_steps_that_draw_nothing() {
        let script = parse("Down*3,Up", "1", "").unwrap();
        assert_eq!(script.keys.len(), 4);
        assert!(parse("Down*x", "1", "").is_err());
        assert_eq!(parse_range("2..10").unwrap(), 2..10);
        assert!(parse_range("2-10").is_err());

        let mut script = parse("Ctrl+C", "1", "").unwrap();
        script.handled = true;
        let mut driver = Driver::new(script);
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.send(&mut ctx);
        driver.arrived(Step::Key(ctx.posted_key().unwrap()));
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.check_frame(false, false, &mut ctx);
        assert_eq!(driver.times[0].len(), 1, "timed until handled");
        assert!(ctx.close_requested());
    }

    #[test]
    fn quoted_text_is_typed_one_character_per_step() {
        let script = parse("'x'", "1", r"Ctrl+F, 'a\d{1,4}', Alt+Enter").unwrap();
        assert_eq!(script.keys, [Step::Char('x')]);
        let typed: String = script
            .setup
            .iter()
            .filter_map(|s| match s {
                Step::Char(ch) => Some(*ch),
                Step::Key(_) => None,
            })
            .collect();
        assert_eq!(typed, r"a\d{1,4}");
        assert_eq!(script.setup.len(), 10);
        assert!(matches!(script.setup[9], Step::Key(k) if k.alt));
        assert!(parse("'x", "1", "").is_err());
        assert!(parse("''", "1", "").is_err());
        assert!(parse("'x' F3", "1", "").is_err());

        let mut driver = Driver::new(script);
        driver.setup = driver.script.setup.len();
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.send(&mut ctx);
        assert_eq!(ctx.posted_char(), Some('x'));
        assert!(ctx.posted_key().is_none());
    }

    #[test]
    fn setup_keys_are_untimed_and_busy_keys_wait_for_their_last_frame() {
        let script = parse("F3", "1", "Ctrl+F").unwrap();
        let mut driver = Driver::new(script);
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.send(&mut ctx);
        let setup = ctx.posted_key().unwrap();
        assert_eq!(setup.vk, 0x46);
        driver.arrived(Step::Key(setup));
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.check_frame(false, false, &mut ctx);
        let key = ctx.posted_key().unwrap();
        assert_eq!(key.vk, 0x72, "setup without a frame moves on");
        driver.arrived(Step::Key(key));
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.check_frame(true, true, &mut ctx);
        driver.presented(true, &mut ctx);
        assert!(driver.times[0].is_empty(), "still busy");
        driver.check_frame(false, false, &mut ctx);
        assert_eq!(driver.times[0].len(), 1);
        assert!(ctx.close_requested());
    }

    #[test]
    fn the_driver_times_each_key_and_closes_after_the_last_run() {
        let script = parse("Ctrl+P,Escape", "2", "").unwrap();
        let keys = script.keys.clone();
        let mut driver = Driver::new(script);
        let mut ctx = Ctx::headless(10, 10, 96);
        driver.send(&mut ctx);
        let mut posted = ctx.posted_key();
        for _ in 0..4 {
            let key = posted.unwrap();
            driver.arrived(Step::Key(key));
            let mut ctx = Ctx::headless(10, 10, 96);
            driver.check_frame(true, false, &mut ctx);
            driver.presented(false, &mut ctx);
            posted = ctx.posted_key();
            if posted.is_none() {
                assert!(ctx.close_requested());
            }
        }
        assert_eq!(driver.times[0].len(), 2);
        assert_eq!(driver.times[1].len(), 2);
        assert_eq!(key_of(keys[1]).vk, 0x1B);
    }
}
