import { chmod, open } from "node:fs/promises";

async function rolledBack(rollback, error) {
  try {
    await rollback();
  } catch (error_) {
    console.error(`herald: the rollback after a failed transaction failed too: ${error_?.message ?? error_}`);
  }
  throw error;
}

export async function sqlite(file, busy) {
  const handle = await open(file, "a", 0o600);
  await handle.close();
  await chmod(file, 0o600);
  const waiting = Number.isSafeInteger(busy) ? `PRAGMA busy_timeout = ${busy}` : undefined;
  if (globalThis.Bun) {
    const { Database } = await import("bun:sqlite");
    const db = new Database(file);
    if (waiting) db.exec(waiting);
    const run = (statement) => {
      const prepared = db.prepare(statement.sql);
      if (prepared.columnNames.length) return { rows: prepared.values(...statement.params), changed: 0 };
      return { rows: [], changed: prepared.run(...statement.params).changes };
    };
    return { transaction: async (statements) => db.transaction(() => statements.map(run)).immediate() };
  }
  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(file);
  if (waiting) db.exec(waiting);
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
        return rolledBack(() => db.exec("ROLLBACK"), error);
      }
    },
  };
}

const answered = (result) =>
  result.fields?.length ? { rows: result.rows, changed: 0 } : { rows: [], changed: result.rowCount ?? 0 };

export async function postgres({ url, connect, statement }, driver = () => import("pg")) {
  const { default: pg } = await driver();
  const pool = new pg.Pool({
    connectionString: url,
    max: 1,
    connectionTimeoutMillis: connect,
    statement_timeout: statement,
    allowExitOnIdle: true,
  });
  pool.on("error", (error) => console.error(`herald: the database closed its connection: ${error.message}`));
  return {
    async transaction(statements) {
      const client = await pool.connect();
      let broken;
      try {
        await client.query("BEGIN");
        const answers = await statements.reduce(
          async (earlier, statement) => [
            ...(await earlier),
            answered(await client.query({ text: statement.sql, values: statement.params, rowMode: "array" })),
          ],
          Promise.resolve([]),
        );
        await client.query("COMMIT");
        return answers;
      } catch (error) {
        broken = error;
        return await rolledBack(() => client.query("ROLLBACK"), error);
      } finally {
        client.release(broken);
      }
    },
  };
}
