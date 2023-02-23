use rusqlite::{MappedRows, Params, Result, Row, Statement};
use std::iter::Map;

/// A struct that owns a [Statement] and has an `iter` method to iterate over the
/// results of that DB query statement.
pub struct QueryIterator<'db, P, T> {
    stmt: Statement<'db>,
    params_and_mapper: Option<(P, Box<dyn Fn(&Row) -> T>)>,
}

impl<'db, P, T> QueryIterator<'db, P, T>
where
    P: Params,
{
    /// Construct a new [QueryIterator].
    pub fn new(stmt: Statement<'db>, params: P, f: impl Fn(&Row) -> T + 'static) -> Self {
        Self {
            stmt,
            params_and_mapper: Some((params, Box::new(f))),
        }
    }

    /// Returns an iterator over the results of the query.
    ///
    /// This method should be called only once per [QueryIterator] and then consumed.
    /// After calling this method, subsequent calls will return [None].
    pub fn iter(
        &mut self,
    ) -> Option<Map<MappedRows<'_, impl FnMut(&Row) -> Result<T>>, impl FnMut(Result<T>) -> T>>
    {
        self.params_and_mapper.take().map(move |(params, mapper)| {
            self.stmt
                .query_map(params, move |row| Ok((mapper)(row)))
                .unwrap()
                .map(|row| row.unwrap())
        })
    }
}
