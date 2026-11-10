export function d1(db) {
  return {
    async transaction(statements) {
      const results = await db.batch(statements.map((statement) => db.prepare(statement.sql).bind(...statement.params)));
      return results.map((result) => ({
        rows: (result.results ?? []).map((row) => Object.values(row)),
        changed: result.meta?.changes ?? 0,
      }));
    },
  };
}

export function transacted(storage, statements) {
  const sql = storage.sql;
  const total = () => sql.exec("SELECT total_changes() AS total").one().total;
  return storage.transactionSync(() =>
    statements.map((statement) => {
      const before = total();
      const rows = sql.exec(statement.sql, ...statement.params).raw().toArray();
      const changed = total() === before ? 0 : sql.exec("SELECT changes() AS changed").one().changed;
      return { rows, changed };
    }),
  );
}
