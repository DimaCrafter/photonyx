use std::fmt;
use crate::{diff::Op, schema::*};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerKind {
	DropTable,
	DropColumn,
	/// Shorter string, smaller integer, removed enum values
	Narrowing,
	/// Type change that is neither widening nor narrowing of the same kind
	TypeChange,
	/// `NOT NULL` column without `DEFAULT` on an existing table
	NotNullWithoutDefault
}

impl fmt::Display for DangerKind {
	fn fmt (&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		return f.write_str(match self {
			DangerKind::DropTable => "DROP TABLE",
			DangerKind::DropColumn => "DROP COLUMN",
			DangerKind::Narrowing => "NARROWING",
			DangerKind::TypeChange => "TYPE CHANGE",
			DangerKind::NotNullWithoutDefault => "NOT NULL WITHOUT DEFAULT"
		});
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Danger {
	pub op_idx: usize,
	pub kind: DangerKind,
	pub table: String,
	pub details: String
}

impl Danger {
	pub fn describe (&self) -> String {
		if self.details.is_empty() {
			return format!("{} {}", self.kind, self.table);
		}

		return format!("{} {}.{}", self.kind, self.table, self.details);
	}
}

pub fn analyze (ops: &[Op]) -> Vec<Danger> {
	let mut result = Vec::new();
	for (index, op) in ops.iter().enumerate() {
		let mut push = |kind: DangerKind, details: String|
			result.push(Danger {
				op_idx: index,
				kind,
				table: op.get_table().to_owned(),
				details
			});

		match op {
			Op::DropTable { .. } => {
				push(DangerKind::DropTable, String::new());
			},
			Op::DropColumn { column, .. } => {
				push(DangerKind::DropColumn, column.clone());
			},
			Op::AddColumn { column, .. } => {
				if !column.nullable && column.default.is_none() {
					push(DangerKind::NotNullWithoutDefault, column.name.clone());
				}
			}
			Op::ModifyColumn { from, to, .. } => {
				match compare_types(&from.db_type, &to.db_type) {
					TypeChange::Same | TypeChange::Widening => {}
					TypeChange::Narrowing => push(DangerKind::Narrowing, format!("{} {} --> {}", to.name, from.db_type, to.db_type)),
					TypeChange::Other => push(DangerKind::TypeChange, format!("{} {} --> {}", to.name, from.db_type, to.db_type))
				}

				if from.nullable && !to.nullable && to.default.is_none() {
					push(DangerKind::NotNullWithoutDefault, to.name.clone());
				}
			}
			Op::RecreateEnumType { from, to, .. } => {
				let removed: Vec<&str> = from
					.iter()
					.filter(|value| !to.contains(value))
					.map(String::as_str)
					.collect();

				if removed.is_empty() {
					push(DangerKind::TypeChange, format!("values reordered ({} --> {})", from.join(", "), to.join(", ")));
				} else {
					push(DangerKind::Narrowing, format!("values removed: {}", removed.join(", ")));
				}
			}
			_ => {}
		}
	}

	return result;
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeChange {
	Same,
	Widening,
	Narrowing,
	Other
}

pub fn compare_types (from: &ColumnType, to: &ColumnType) -> TypeChange {
	use ColumnType::*;

	if from == to {
		return TypeChange::Same;
	}

	let wider = |ok: bool| if ok { TypeChange::Widening } else { TypeChange::Narrowing };

	return match (from, to) {
		(Int { size: a, unsigned: ua }, Int { size: b, unsigned: ub }) => {
			let fits = match (ua, ub) {
				// Signed never fits unsigned
				(false, true) => false,
				// Unsigned fits bigger signed
				(true, false) => b > a,
				_ => b >= a
			};

			wider(fits)
		}

		(Bool, Int { .. }) => TypeChange::Widening,
		(Int { .. }, Bool) => TypeChange::Narrowing,
		(Float, Double) => TypeChange::Widening,
		(Double, Float) => TypeChange::Narrowing,

		(Varchar(a), Varchar(b))
		| (Char(a), Char(b))
		| (Char(a), Varchar(b)) => wider(b >= a),

		(Varchar(_) | Char(_), Text) => TypeChange::Widening,
		(Text, Varchar(_) | Char(_)) => TypeChange::Narrowing,

		(Enum(a), Enum(b)) => {
			wider(a.iter().all(|value| b.contains(value)))
		},
		(Enum(values), Varchar(size) | Char(size)) => {
			wider(values.iter().all(|value| value.chars().count() <= *size as usize))
		},
		(Enum(_), Text) => {
			TypeChange::Widening
		}
		_ => TypeChange::Other
	};
}
