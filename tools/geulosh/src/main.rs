//! geulosh: GeulOS 인터랙티브 셸 바이너리.

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use geulos_shell::{Shell, ShellOutcome};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    let mut shell = Shell::new();

    // --script <path> 모드 분기
    if let Some(script_path) = parse_script_flag(&args) {
        match run_script(&mut shell, &script_path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("script failed: {}", msg);
                ExitCode::from(1)
            }
        }
    } else {
        // 인터랙티브 REPL
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut out = stdout.lock();

        let _ = writeln!(out, "geulosh — GeulOS interactive shell. Type `help` or `exit`.");
        let _ = out.flush();

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let outcome = shell.execute(&line);
            match outcome {
                ShellOutcome::Output(s) => {
                    let _ = writeln!(out, "{}", s);
                }
                ShellOutcome::Error(e) => {
                    let _ = writeln!(out, "error: {}", e);
                }
                ShellOutcome::Quit => return ExitCode::SUCCESS,
                ShellOutcome::NoOp => {}
            }
            let _ = out.flush();
        }
        ExitCode::SUCCESS
    }
}

fn parse_script_flag(args: &[String]) -> Option<String> {
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--script" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

fn run_script(shell: &mut Shell, path: &str) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))?;
    let mut last_output: String = String::new();
    let mut last_was_error: bool = false;

    for (lineno, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // 디렉티브 처리
        if let Some(rest) = line.strip_prefix("expect ") {
            let needle = rest.trim().trim_matches('"');
            if !last_output.contains(needle) {
                return Err(format!(
                    "line {}: expect '{}' — 직전 출력에 없음:\n--- 출력 ---\n{}\n------------",
                    lineno + 1,
                    needle,
                    last_output
                ));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("expect-error ") {
            let needle = rest.trim().trim_matches('"');
            if !last_was_error || !last_output.contains(needle) {
                return Err(format!(
                    "line {}: expect-error '{}' — 직전이 에러가 아니거나 메시지 불일치:\n{}",
                    lineno + 1,
                    needle,
                    last_output
                ));
            }
            continue;
        }
        if line == "assert success" {
            if last_was_error {
                return Err(format!(
                    "line {}: assert success — 직전이 에러였음: {}",
                    lineno + 1,
                    last_output
                ));
            }
            continue;
        }
        if line == "assert error" {
            if !last_was_error {
                return Err(format!(
                    "line {}: assert error — 직전이 성공이었음: {}",
                    lineno + 1,
                    last_output
                ));
            }
            continue;
        }

        // 일반 명령
        match shell.execute(line) {
            ShellOutcome::Output(s) => {
                println!("{}", s);
                last_output = s;
                last_was_error = false;
            }
            ShellOutcome::Error(e) => {
                println!("error: {}", e);
                last_output = e;
                last_was_error = true;
            }
            ShellOutcome::Quit => return Ok(()),
            ShellOutcome::NoOp => {
                last_output.clear();
                last_was_error = false;
            }
        }
    }
    Ok(())
}
