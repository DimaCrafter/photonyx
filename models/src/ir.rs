use std::{fmt, path::PathBuf};
pub use crate::runtime::FieldType;


/// 1-based line and column
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
	pub line: usize,
	pub col: usize
}

#[derive(Debug, Clone)]
pub struct ModelSet {
	pub groups: Vec<Group>
}

#[derive(Debug, Clone)]
pub struct Group {
	pub name: String,
	pub dir: PathBuf,
	pub entities: Vec<Entity>
}

impl Group {
	pub fn new_detached (name: String) -> Self {
		return Group {
			name,
			dir: PathBuf::new(),
			entities: Vec::new()
		};
	}
}

#[derive(Debug, Clone)]
pub struct Entity {
	pub name: String,
	pub file: PathBuf,
	pub kind: EntityKind
}

#[derive(Debug, Clone)]
pub enum EntityKind {
	Table(Table),
	Enum(Enum)
}

#[derive(Debug, Clone)]
pub struct Table {
	pub table: String,
	pub fields: Vec<Field>,
	pub indexes: Vec<Index>,
	/// `true` when the table is going to be dropped by a migration
	pub deleted: bool,
	pub dialects: Vec<DialectBlock>
}

#[derive(Debug, Clone)]
pub struct Field {
	pub name: String,
	/// Column name when it differs from the field name
	pub column: Option<String>,
	pub field_type: FieldType,
	pub nullable: bool,
	pub enum_ref: Option<EntityRef>,
	pub default: Option<Default>,
	// TODO: extend update triggers
	/// `update: now`
	pub update_now: bool,
	pub auto: bool,
	pub dialects: Vec<DialectBlock>,
	pub pos: Pos
}

impl Field {
	pub fn column_name (&self) -> &str {
		return self.column.as_deref().unwrap_or(&self.name);
	}
}

#[derive(Debug, Clone, PartialEq)]
pub enum Default {
	Int(i64),
	Float(f64),
	Bool(bool),
	Str(String),
	/// `CURRENT_TIMESTAMP`
	Now
}

#[derive(Debug, Clone)]
pub struct Index {
	pub name: String,
	pub columns: Vec<String>,
	pub primary: bool,
	pub unique: bool,
	pub link: Option<Link>,
	/// Name of the FK constraint, if it differs from the index name
	pub constraint: Option<String>,
	pub dialects: Vec<DialectBlock>,
	pub pos: Pos
}

#[derive(Debug, Clone)]
pub struct Link {
	pub target: EntityRef,
	/// `Model.index`; `None` - primary key of the target
	pub index: Option<String>,
	pub delete: DeletePolicy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletePolicy {
	Restrict,
	Cascade,
	SetNull,
	None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRef {
	pub group: Option<String>,
	pub name: String
}

impl EntityRef {
	pub fn parse (raw: &str) -> Self {
		if let Some((group, name)) = raw.split_once('.') {
			return EntityRef { group: Some(group.to_owned()), name: name.to_owned() };
		}

		return EntityRef { group: None, name: raw.to_owned() };
	}

	pub fn group_or<'a> (&'a self, current: &'a str) -> &'a str {
		return self.group.as_deref().unwrap_or(current);
	}
}

impl fmt::Display for EntityRef {
	fn fmt (&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if let Some(group) = &self.group {
			return write!(f, "{group}.{}", self.name);
		}

		return f.write_str(&self.name);
	}
}

#[derive(Debug, Clone)]
pub struct Enum {
	pub values: Vec<EnumValue>
}

#[derive(Debug, Clone)]
pub struct EnumValue {
	pub name: String,
	pub int: Option<i64>,
	/// String representation in the database, if it differs from the name
	pub db: Option<String>,
	pub doc: Option<String>,
	pub pos: Pos
}

impl EnumValue {
	pub fn db_name (&self) -> &str {
		return self.db.as_deref().unwrap_or(&self.name);
	}
}

#[derive(Debug, Clone)]
pub struct DialectBlock {
	pub dialect: String,
	pub value: Raw
}

#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
	Null,
	Bool(bool),
	Int(i64),
	Float(f64),
	Str(String),
	Seq(Vec<Raw>),
	Map(Vec<(String, Raw)>)
}


impl ModelSet {
	pub fn find_group (&self, name: &str) -> Option<&Group> {
		return self.groups.iter().find(|group| group.name == name);
	}

	pub fn find_entity (&self, current_group: &str, target: &EntityRef) -> Option<&Entity> {
		return self.find_group(target.group_or(current_group))?.find_entity(&target.name);
	}
}

impl Group {
	pub fn find_entity (&self, name: &str) -> Option<&Entity> {
		return self.entities.iter().find(|entity| entity.name == name);
	}
}

impl Table {
	pub fn find_field (&self, name: &str) -> Option<&Field> {
		return self.fields.iter().find(|field| field.name == name);
	}

	pub fn primary (&self) -> Option<&Index> {
		return self.indexes.iter().find(|index| index.primary);
	}

	pub fn find_index (&self, name: &str) -> Option<&Index> {
		return self.indexes.iter().find(|index| index.name == name);
	}
}
