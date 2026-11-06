import { chmod, open } from "node:fs/promises";

export async function sqlite(file) {
  const handle = await open(file, "a", 0o600);
  await handle.close();
  await chmod(file, 0o600);
  if (globalThis.Bun) {
    const { Database } = await import("bun:sqlite");
    const db = new Database(file);
    const run = (statement) => {
      const prepared = db.prepare(statement.sql);
      if (prepared.columnNames.length) return { rows: prepared.values(...statement.params), changed: 0 };
      return { rows: [], changed: prepared.run(...statement.params).changes };
    };
    return { transaction: async (statements) => db.transaction(() => statements.map(run)).immediate() };
  }
  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(file);
  const run = (statement) => {
    const prepared = db.prepare(statement.sql);
    if (prepared.columns().length) {
      prepared.setReturnArrays(true);
      return { rows: prepared.all(...statement.params), changed: 0 };
    }
    return { rows: [], changed: Number(prepared.run(...statement.params).changes) };
  };
  return {
    async transaction(statements) {
      db.exec("BEGIN IMMEDIATE");
      try {
        const answers = statements.map(run);
        db.exec("COMMIT");
        return answers;
      } catch (error) {
        db.exec("ROLLBACK");
        throw error;
      }
    },
  };
}

export async function postgres(url) {
  const { default: pg } = await import("pg");
  const client = new pg.Client({ connectionString: url });
  await client.connect();
  return {
    async transaction(statements) {
      await client.query("BEGIN");
      try {
        const results = await Promise.all(
          statements.map((statement) => client.query({ text: statement.sql, values: statement.params, rowMode: "array" })),
        );
        await client.query("COMMIT");
        return results.map((result) => ({ rows: result.rows ?? [], changed: result.rowCount ?? 0 }));
      } catch (error) {
        await client.query("ROLLBACK");
        throw error;
      }
    },
  };
}
