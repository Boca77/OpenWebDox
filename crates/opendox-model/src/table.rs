//! Tables: a placeholder character in the body text, plus a table container holding columns and
//! rows.
//!
//! The body holds one U+FFFC (OBJECT REPLACEMENT CHARACTER) per table, marked with the `table`
//! key whose value is the table id. The table itself lives in the `tables` map under that id:
//!
//! ```text
//! tables[id] = {
//!     columns: MovableList<column id>,
//!     rows:    MovableList<Map<column id, Text>>,
//! }
//! ```
//!
//! Cells are keyed by column id, not by index, so a column added or removed concurrently with a
//! row addition still lines up: a row missing a cell for some column reads as an empty cell, and
//! cells for removed columns are ignored. Keeping the table's place as a body character means
//! splitting paragraphs and typing around a table stay ordinary, concurrency-safe text edits.

use loro::{Container, LoroMap, LoroMovableList, LoroText, LoroValue, ValueOrContainer};

/// The character standing in for a table in the body text.
pub const TABLE_PLACEHOLDER: char = '\u{FFFC}';

/// Mark key on a placeholder; its value is the table id.
pub(crate) const TABLE_MARK: &str = "table";

pub(crate) const TABLES: &str = "tables";
const COLUMNS: &str = "columns";
const ROWS: &str = "rows";

/// A table cell, addressed by its current row and column index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub table: String,
    pub row: usize,
    pub col: usize,
}

/// Handle to one table's containers.
pub(crate) struct TableHandle {
    pub(crate) map: LoroMap,
}

impl TableHandle {
    pub(crate) fn columns_list(&self) -> Option<LoroMovableList> {
        match self.map.get(COLUMNS)? {
            ValueOrContainer::Container(Container::MovableList(l)) => Some(l),
            _ => None,
        }
    }

    pub(crate) fn rows_list(&self) -> Option<LoroMovableList> {
        match self.map.get(ROWS)? {
            ValueOrContainer::Container(Container::MovableList(l)) => Some(l),
            _ => None,
        }
    }

    /// Column ids in order.
    pub(crate) fn columns(&self) -> Vec<String> {
        let Some(list) = self.columns_list() else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|i| match list.get(i)? {
                ValueOrContainer::Value(v) => v.as_string().map(|s| s.to_string()),
                ValueOrContainer::Container(_) => None,
            })
            .collect()
    }

    /// Row maps in order.
    pub(crate) fn rows(&self) -> Vec<LoroMap> {
        let Some(list) = self.rows_list() else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|i| match list.get(i)? {
                ValueOrContainer::Container(Container::Map(m)) => Some(m),
                _ => None,
            })
            .collect()
    }

    /// Cell texts, row by row, one entry per current column. Missing cells read as empty.
    pub(crate) fn cell_texts(&self) -> Vec<Vec<String>> {
        let columns = self.columns();
        self.rows()
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|c| cell_container(row, c).map_or_else(String::new, |t| t.to_string()))
                    .collect()
            })
            .collect()
    }

    /// Initialises an empty table's containers.
    pub(crate) fn init(&self, column_ids: &[String], rows: usize) -> loro::LoroResult<()> {
        let columns = self.map.insert_container(COLUMNS, LoroMovableList::new())?;
        for id in column_ids {
            columns.push(LoroValue::from(id.as_str()))?;
        }
        let row_list = self.map.insert_container(ROWS, LoroMovableList::new())?;
        for _ in 0..rows {
            let row = row_list.push_container(LoroMap::new())?;
            for id in column_ids {
                row.insert_container(id, LoroText::new())?;
            }
        }
        Ok(())
    }
}

pub(crate) fn cell_container(row: &LoroMap, column: &str) -> Option<LoroText> {
    match row.get(column)? {
        ValueOrContainer::Container(Container::Text(t)) => Some(t),
        _ => None,
    }
}
