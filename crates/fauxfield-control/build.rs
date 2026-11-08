//! Emits build metadata for `build_info`. Uses the `git` CLI and never fails the build: any
//! problem yields "unknown". Honours SOURCE_DATE_EPOCH, never the wall clock.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Days-since-epoch to `YYYY-MM-DD` (Howard Hinnant's civil_from_days).
fn civil(epoch: i64) -> String {
    let z = epoch.div_euclid(86400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    let sha = git(&["rev-parse", "--short=7", "HEAD"]);
    let date = git(&["log", "-1", "--format=%cs"]);
    let dirty = match git(&["status", "--porcelain"]) {
        Some(_) => "1",
        None if sha.is_some() => "0",
        None => "0",
    };
    let date = match std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
    {
        Some(e) => date.map(|_| civil(e)).or_else(|| Some(civil(e))),
        None => date,
    };
    let (sha, date) = match (sha, date) {
        (Some(s), Some(d)) => (s, d),
        _ => ("unknown".to_string(), "unknown".to_string()),
    };
    println!("cargo:rustc-env=FAUXFIELD_GIT_SHA={sha}");
    println!("cargo:rustc-env=FAUXFIELD_GIT_DATE={date}");
    println!("cargo:rustc-env=FAUXFIELD_GIT_DIRTY={dirty}");
    println!(
        "cargo:rustc-env=FAUXFIELD_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".into())
    );
    println!(
        "cargo:rustc-env=FAUXFIELD_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".into())
    );
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    if let Some(g) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={g}/HEAD");
        println!("cargo:rerun-if-changed={g}/index");
    }
}
