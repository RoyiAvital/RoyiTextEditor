//! Development tasks. Run through the Cargo alias: `cargo xtask <task>`.
//!
//! - `bench-startup [--runs N] [--warmup N] [--no-gate] [FILE]`: builds the release binary,
//!   launches it repeatedly with `--trace-startup --exit-after-first-frame`
//!   and reports the startup stages (median, p95). The gate is p95 of
//!   `first_frame` < 100 ms. Without `FILE` a generated C file is used.
//!   Any run (warm-up included) that fails, times out or prints an invalid
//!   trace fails the task.
//! - `check-imports [EXE]`: lists the DLLs the executable imports and fails
//!   on MinGW runtime DLLs (the release must be self-contained), and checks
//!   that the application manifest is embedded.
//! - `bench-perf [--short] [--print-hashes]`: Performance Benchmark v1 (`perf.rs`,
//!   `Perfromance.md`); `--print-hashes` regenerates the fixtures and prints
//!   their hashes.
//! - `package`: builds the release, checks imports and writes
//!   `target/package/<name>-<version>-windows-x86_64.zip` (`Meta/AppMeta.json`); prints its path last.

mod perf;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const GATE_MS: f64 = 100.0;
/// Upper bound of one traced application run.
const RUN_TIMEOUT: Duration = Duration::from_secs(10);
const APP: &str = "RoyiTextEditor";
const FORBIDDEN_DLLS: &[&str] = &[
    "libgcc_s_seh-1.dll",
    "libwinpthread-1.dll",
    "libstdc++-6.dll",
];
const MANIFEST_MARK: &[u8] = b"<dpiAwareness";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bench-startup") => bench_startup(&args[1..]),
        Some("bench-perf") => perf::bench_perf(&args[1..]),
        Some("check-imports") => {
            let exe = args.get(1).map_or_else(release_exe, PathBuf::from);
            if check_imports(&exe) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some("package") => package(),
        _ => {
            eprintln!(
                "usage: cargo xtask <bench-startup [--runs N] [--warmup N] [FILE] | bench-perf [--short] [--print-hashes] | check-imports [EXE] | package>"
            );
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn release_exe() -> PathBuf {
    root()
        .join("target")
        .join("release")
        .join(format!("{APP}.exe"))
}

/// `(name, version)` from `Meta/AppMeta.json`.
fn app_info() -> Result<(String, String), String> {
    let path = root().join("Meta").join("AppMeta.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let info: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let field = |key: &str| {
        info[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{}: no string field `{key}`", path.display()))
    };
    Ok((field("name")?, field("version")?))
}

fn build_release() -> bool {
    Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "rte_app"])
        .current_dir(root())
        .status()
        .is_ok_and(|s| s.success())
}

fn check_imports(exe: &Path) -> bool {
    let Ok(bytes) = std::fs::read(exe) else {
        eprintln!("cannot read {}", exe.display());
        return false;
    };
    let dlls = match imported_dlls(&bytes) {
        Some(dlls) => dlls,
        None => {
            eprintln!("cannot read the import table of {}", exe.display());
            return false;
        }
    };
    println!("imports of {}: {}", exe.display(), dlls.join(", "));
    // The manifest resource is stored as plain UTF-8 XML.
    let manifest = bytes
        .windows(MANIFEST_MARK.len())
        .any(|w| w == MANIFEST_MARK);
    if !manifest {
        eprintln!("the application manifest is not embedded");
    }
    let bad: Vec<&String> = dlls
        .iter()
        .filter(|d| FORBIDDEN_DLLS.iter().any(|f| d.eq_ignore_ascii_case(f)))
        .collect();
    if !bad.is_empty() {
        eprintln!("forbidden MinGW runtime imports: {bad:?}");
    }
    manifest && bad.is_empty()
}

/// DLL names from the import directory of a PE32+ image.
fn imported_dlls(pe: &[u8]) -> Option<Vec<String>> {
    let u16_at = |o: usize| pe.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        pe.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let pe_header = u32_at(0x3C)? as usize;
    if pe.get(pe_header..pe_header + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe_header + 4;
    let sections = u16_at(coff + 2)? as usize;
    let optional = coff + 20;
    if u16_at(optional)? != 0x20B {
        return None;
    }
    let import_rva = u32_at(optional + 112 + 8)?;
    let section_table = optional + u16_at(coff + 16)? as usize;
    let to_offset = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|i| {
            let s = section_table + i * 40;
            let (vsize, va, raw_size, raw) = (
                u32_at(s + 8)?,
                u32_at(s + 12)?,
                u32_at(s + 16)?,
                u32_at(s + 20)?,
            );
            (rva >= va && rva < va + vsize.max(raw_size)).then(|| (raw + rva - va) as usize)
        })
    };
    let mut dlls = Vec::new();
    let mut descriptor = to_offset(import_rva)?;
    loop {
        let name_rva = u32_at(descriptor + 12)?;
        if name_rva == 0 {
            break;
        }
        let start = to_offset(name_rva)?;
        let end = start + pe.get(start..)?.iter().position(|&b| b == 0)?;
        dlls.push(String::from_utf8_lossy(&pe[start..end]).into_owned());
        descriptor += 20;
    }
    Some(dlls)
}

fn package() -> ExitCode {
    if !build_release() {
        eprintln!("release build failed");
        return ExitCode::FAILURE;
    }
    let exe = release_exe();
    if !check_imports(&exe) {
        return ExitCode::FAILURE;
    }
    let (app, version) = match app_info() {
        Ok(info) => info,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let name = format!("{app}-{version}-windows-x86_64");
    let out = root().join("target").join("package");
    let stage = out.join(&name);
    let _ = std::fs::remove_dir_all(&stage);
    let copied = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&stage)?;
        std::fs::copy(&exe, stage.join(format!("{APP}.exe")))?;
        copy_dir(&root().join("assets").join("fonts"), &stage.join("Fonts"))?;
        for doc in ["README.md", "CHANGELOG.md"] {
            std::fs::copy(root().join(doc), stage.join(doc))?;
        }
        Ok(())
    })();
    if let Err(e) = copied {
        eprintln!("staging failed: {e}");
        return ExitCode::FAILURE;
    }
    let archive = out.join(format!("{name}.zip"));
    let _ = std::fs::remove_file(&archive);
    // Windows' bsdtar writes ZIP archives with `-a`.
    let zipped = Command::new("tar")
        .arg("-a")
        .arg("-c")
        .arg("-f")
        .arg(&archive)
        .arg("-C")
        .arg(&out)
        .arg(&name)
        .status()
        .is_ok_and(|s| s.success());
    if !zipped {
        eprintln!("creating {} failed", archive.display());
        return ExitCode::FAILURE;
    }
    println!("{}", archive.display());
    ExitCode::SUCCESS
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn bench_startup(args: &[String]) -> ExitCode {
    let mut runs = 50usize;
    let mut warmup = 5usize;
    let mut gate = true;
    let mut file: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--runs" => runs = it.next().and_then(|v| v.parse().ok()).unwrap_or(runs),
            "--warmup" => warmup = it.next().and_then(|v| v.parse().ok()).unwrap_or(warmup),
            "--no-gate" => gate = false,
            path => file = Some(PathBuf::from(path)),
        }
    }

    let status = build_release();
    if !status {
        eprintln!("release build failed");
        return ExitCode::FAILURE;
    }
    let exe = release_exe();
    let file = file.unwrap_or_else(sample_c_file);

    let mut stages: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for i in 0..warmup + runs {
        let mut command = Command::new(&exe);
        command
            .args(["--trace-startup", "--exit-after-first-frame"])
            .arg(&file);
        let trace = match run_traced(&mut command, RUN_TIMEOUT).and_then(|out| parse_trace(&out)) {
            Ok(trace) => trace,
            Err(e) => {
                eprintln!("run {} of {}: {e}", i + 1, warmup + runs);
                return ExitCode::FAILURE;
            }
        };
        if i < warmup {
            continue;
        }
        for (key, ms) in trace {
            if !key.ends_with(MEMORY_SUFFIX) {
                stages.entry(key).or_default().push(ms);
            }
        }
    }

    println!("file: {}  runs: {runs}  warm-up: {warmup}", file.display());
    println!(
        "{:<16} {:>9} {:>9} {:>9} {:>9}",
        "stage [ms]", "min", "median", "p95", "max"
    );
    let mut ordered: Vec<_> = stages.into_iter().collect();
    ordered.sort_by(|a, b| median(&a.1).total_cmp(&median(&b.1)));
    let mut p95_first = None;
    for (stage, mut values) in ordered {
        values.sort_by(f64::total_cmp);
        let p95 = percentile(&values, 0.95);
        if stage == "first_frame" {
            p95_first = Some(p95);
        }
        println!(
            "{stage:<16} {:>9.2} {:>9.2} {p95:>9.2} {:>9.2}",
            values[0],
            percentile(&values, 0.5),
            values[values.len() - 1]
        );
    }
    match p95_first {
        Some(p95) if !gate => {
            println!("gate: not applied (first_frame p95 {p95:.2} ms)");
            ExitCode::SUCCESS
        }
        Some(p95) if p95 < GATE_MS => {
            println!("gate: PASS (first_frame p95 {p95:.2} ms < {GATE_MS} ms)");
            ExitCode::SUCCESS
        }
        Some(p95) => {
            println!("gate: FAIL (first_frame p95 {p95:.2} ms >= {GATE_MS} ms)");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("no first_frame measurements");
            ExitCode::FAILURE
        }
    }
}

/// Runs `command` and returns its standard output. Launch failures, a
/// timeout (the child is killed) and unsuccessful exits are errors.
fn run_traced(command: &mut Command, timeout: Duration) -> Result<String, String> {
    fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("launch failed: {e}"))?;
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => thread::sleep(Duration::from_millis(1)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} ms", timeout.as_millis()));
            }
            Err(e) => return Err(format!("waiting failed: {e}")),
        }
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("exited with {status}; stderr: {}", stderr.trim()));
    }
    Ok(stdout)
}

/// Key suffix of memory values in a parsed trace.
const MEMORY_SUFFIX: &str = "_mib";

/// Parses `startup.<stage>_ms=<value>` lines and `memory.<name>_mib=<value>`
/// lines (keyed `<name>_mib`). Every non-empty line must be one of them with
/// a finite, non-negative value; keys must be unique and `first_frame` present.
fn parse_trace(stdout: &str) -> Result<BTreeMap<String, f64>, String> {
    let mut stages = BTreeMap::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (key, value) = if let Some(memory) = line.strip_prefix("memory.") {
            memory
                .split_once('=')
                .filter(|(key, _)| key.len() > MEMORY_SUFFIX.len() && key.ends_with(MEMORY_SUFFIX))
        } else {
            line.strip_prefix("startup.")
                .and_then(|l| l.split_once("_ms="))
                .filter(|(key, _)| !key.is_empty())
        }
        .ok_or_else(|| format!("unexpected output line: {line:?}"))?;
        let ms = value
            .parse::<f64>()
            .ok()
            .filter(|ms| ms.is_finite() && *ms >= 0.0)
            .ok_or_else(|| format!("invalid value in {line:?}"))?;
        if stages.insert(key.to_owned(), ms).is_some() {
            return Err(format!("duplicate stage {key:?}"));
        }
    }
    if !stages.contains_key("first_frame") {
        return Err("no first_frame measurement".to_owned());
    }
    Ok(stages)
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    percentile(&v, 0.5)
}

/// Nearest-rank percentile of sorted values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = (p * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

/// A ~300-line C file under `target/xtask/`.
fn sample_c_file() -> PathBuf {
    let dir = root().join("target").join("xtask");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("sample.c");
    let mut text = String::from("#include <stdio.h>\n#include <stdlib.h>\n\n");
    for i in 0..30 {
        text.push_str(&format!(
            "/* Function {i}: sums a scaled range. */\nstatic long sum_{i}(const int *values, size_t count)\n{{\n    long total = 0;\n    for (size_t k = 0; k < count; ++k) {{\n        total += (long)values[k] * {i};\n    }}\n    return total;\n}}\n\n"
        ));
    }
    let _ = std::fs::write(&path, text);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(script: &str) -> Command {
        let mut c = Command::new("cmd");
        c.args(["/d", "/c", script]);
        c
    }

    #[test]
    fn successful_run_returns_its_trace() {
        let out = run_traced(
            &mut cmd("echo startup.first_frame_ms=1.5"),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(parse_trace(&out).unwrap()["first_frame"], 1.5);
    }

    #[test]
    fn failing_silent_and_hanging_runs_are_errors() {
        let failed = run_traced(&mut cmd("exit 7"), Duration::from_secs(10)).unwrap_err();
        assert!(failed.contains("exited"), "{failed}");
        let silent = run_traced(&mut cmd("rem"), Duration::from_secs(10)).unwrap();
        assert!(parse_trace(&silent).is_err());
        let mut sleep = Command::new("powershell.exe");
        sleep.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
        let start = Instant::now();
        let hung = run_traced(&mut sleep, Duration::from_millis(300)).unwrap_err();
        assert!(hung.contains("timed out"), "{hung}");
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn invalid_traces_are_rejected() {
        for bad in [
            "",
            "startup.window_ms=1\n",
            "startup.first_frame_ms=1\nstartup.first_frame_ms=2\n",
            "startup.first_frame_ms=x\n",
            "startup.first_frame_ms=-1\n",
            "startup.first_frame_ms=NaN\n",
            "startup.first_frame_ms=inf\n",
            "startup._ms=1\nstartup.first_frame_ms=1\n",
            "panic: something\nstartup.first_frame_ms=1\n",
            "memory.peak=1\nstartup.first_frame_ms=1\n",
            "memory._mib=1\nstartup.first_frame_ms=1\n",
            "memory.peak_private_mib=-2\nstartup.first_frame_ms=1\n",
        ] {
            assert!(parse_trace(bad).is_err(), "{bad:?} was accepted");
        }
        let ok = parse_trace(
            "startup.window_ms=0.5\r\nstartup.first_frame_ms=2\r\nmemory.peak_private_mib=4.75\r\n",
        )
        .unwrap();
        assert_eq!(ok.len(), 3);
        assert_eq!(ok["peak_private_mib"], 4.75);
    }
}
