use std::env;

use crate::utils::sync::{AppStatic, LazyInit};

const RESET: &'static str = "\x1B[0m";
const BOLD: &'static str = "\x1B[1m";
static THEME: AppStatic<ShellTheme> = AppStatic::new();

struct ShellTheme {
	calc_color: fn (ShellColor, u8) -> String
}

impl ShellTheme {
	fn rgb () -> Self {
		ShellTheme {
			calc_color: |color, offset| {
				let code = match color {
					ShellColor::Info => "80;174;248",
					ShellColor::Ok => "0;192;64",
					ShellColor::Warning => "255;112;0",
					ShellColor::Error => "224;0;0"
				};

				return format!("\x1B[{};2;{}m", offset + 8, code);
			}
		}
	}

	fn named () -> Self {
		ShellTheme {
			calc_color: |color, offset| {
				let code = match color {
					ShellColor::Info => "39",
					ShellColor::Ok => "35",
					ShellColor::Warning => "202",
					ShellColor::Error => "160"
				};

				return format!("\x1B[{};5;{}m", offset + 8, code);
			}
		}
	}

	fn ansi () -> Self {
		ShellTheme {
			calc_color: |color, offset| {
				let code = match color {
					ShellColor::Info => 6,
					ShellColor::Ok => 2,
					ShellColor::Warning => 3,
					ShellColor::Error => 1
				};

				return format!("\x1B[{}m", offset + code);
			}
		}
	}
}

impl LazyInit for ShellTheme {
	fn init () -> Self {
		if let Some(palette) = env::var_os("COLORTERM") {
			if palette == "truecolor" || palette == "x24" || palette == "24bit" {
				return Self::rgb();
			} else if palette == "256color" || palette == "ansi256" {
				return Self::named();
			} else {
				return Self::ansi();
			}
		} else {
			return Self::ansi();
		}
	}
}


enum ShellColor { Info, Ok, Warning, Error }

impl ShellColor {
	/// Get foreground color sequence
	#[inline]
	pub fn as_fg (self) -> String {
		return (THEME.calc_color)(self, 30);
	}

	/// Get background color sequence
	#[inline]
	pub fn as_bg (self) -> String {
		return (THEME.calc_color)(self, 40);
	}
}

pub fn log_info (msg: &str) {
	println!("{}{} INFO {} {}", ShellColor::Info.as_bg(), BOLD, RESET, msg);
}

pub fn log_success (msg: &str) {
	println!("{}{} OK {} {}", ShellColor::Ok.as_bg(), BOLD, RESET, msg);
}

pub fn log_warning (msg: &str) {
	println!("{}{} WARN {} {}", ShellColor::Warning.as_bg(), BOLD, RESET, msg);
}

pub fn log_error (msg: &str) {
	println!("{}{} ERR {} {}", ShellColor::Error.as_bg(), BOLD, RESET, msg);
}

pub fn log_error_lines (msg: &str, lines: String) {
	log_error(msg);
	for line in lines.split('\n') {
		println!(" {}│{} {}", ShellColor::Error.as_fg(), RESET, line);
	}

	println!(" {}└─{}", ShellColor::Error.as_fg(), RESET);
}
