use std::io::{stderr, stdout, IsTerminal, Write};


pub struct Log;

fn paint (enabled: bool, code: &str, value: &str) -> String {
	if enabled {
		return format!("\x1B[{code}m{value}\x1B[0m");
	} else {
		return value.to_owned();
	}
}

impl Log {
	pub fn error (text: &str) {
		let mut out = stderr().lock();
		let color = stderr().is_terminal();
		let _ = writeln!(out, "{} {text}", paint(color, "41;97", " ERR "));
	}

	pub fn error_full (text: &str, message: &str) {
		let mut out = stderr().lock();
		let color = stderr().is_terminal();

		let _ = writeln!(out, "{} {text}", paint(color, "41;97", " ERR "));
		for line in message.lines() {
			let _ = writeln!(out, " {} {line}", paint(color, "31", "│"));
		}
		let _ = writeln!(out, " {}", paint(color, "31", "└─"));
	}

	pub fn line (text: &str) {
		let _ = writeln!(stdout().lock(), "{text}");
	}

	pub fn warning (text: &str) -> String {
		return paint(stdout().is_terminal(), "33", text);
	}

	pub fn dimmed (text: &str) -> String {
		return paint(stdout().is_terminal(), "2", text);
	}
}
