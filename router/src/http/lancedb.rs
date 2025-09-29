use lancedb::Table;

#[derive(Clone)]
pub struct LanceDbState {
    pub table: Table,
    pub default_vector_column: Option<String>,
    pub default_columns: Vec<String>,
}

impl LanceDbState {
    pub fn new(
        table: Table,
        default_vector_column: Option<String>,
        default_columns: Vec<String>,
    ) -> Self {
        Self {
            table,
            default_vector_column,
            default_columns,
        }
    }
}
