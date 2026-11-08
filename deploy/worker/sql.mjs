export function transacted(storage, statements) {
  const sql = storage.sql;
  return storage.transactionSync(() =>
    statements.map((statement) => {
      const cursor = sql.exec(statement.sql, ...statement.params);
      const rows = cursor.raw().toArray();
      const changed = cursor.rowsWritten === 0 ? 0 : sql.exec("SELECT changes() AS changed").one().changed;
      return { rows, changed };
    }),
  );
}
