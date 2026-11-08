import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { postgres, sqlite } from "../databases.mjs";

const said = (sql, params = []) => ({ sql, params });

async function directory(t) {
  const made = await mkdtemp(join(tmpdir(), "herald-databases-"));
  t.after(() => rm(made, { recursive: true, force: true }));
  return made;
}

test("a SQLite transaction waits for the lock no longer than it was given", async (t) => {
  const file = join(await directory(t), "herald.sqlite");
  const db = await sqlite(file, 300);
  const holder = new DatabaseSync(file);
  holder.exec("BEGIN IMMEDIATE");
  const started = performance.now();
  await assert.rejects(db.transaction([said("CREATE TABLE t (a INTEGER)")]), /locked/);
  assert.ok(performance.now() - started >= 290);
  holder.exec("COMMIT");
  holder.close();
  const answers = await db.transaction([
    said("CREATE TABLE t (a INTEGER)"),
    said("INSERT INTO t VALUES (?1), (?1)", [7]),
    said("SELECT a FROM t"),
  ]);
  assert.deepEqual(answers[1], { rows: [], changed: 2 });
  assert.deepEqual(answers[2].rows, [[7], [7]]);
});

test("a failed SQLite transaction answers its statement's error, even when the rollback fails", async (t) => {
  const db = await sqlite(join(await directory(t), "herald.sqlite"), 300);
  await assert.rejects(db.transaction([said("NOT SQL")]), /syntax error/);
  t.mock.method(console, "error", () => {});
  await assert.rejects(db.transaction([said("COMMIT"), said("NOT SQL")]), /syntax error/);
  assert.match(console.error.mock.calls[0].arguments[0], /the rollback after a failed transaction failed too/);
});

const url = process.env.HERALD_TEST_POSTGRES;

test("CI runs the Postgres tests against its Postgres service", () => {
  assert.ok(url || !process.env.CI, "HERALD_TEST_POSTGRES is not set");
});

const pg = () => import("pg");
const within = { connect: 5_000, statement: 30_000 };

test("a Postgres statement past its timeout is cancelled", { skip: !url }, async () => {
  const db = await postgres({ url, ...within, statement: 200 }, pg);
  await assert.rejects(db.transaction([said("SELECT pg_sleep(2)")]), /statement timeout/);
  const [answer] = await db.transaction([said("SELECT 1")]);
  assert.deepEqual(answer, { rows: [[1]], changed: 0 });
});

test("a Postgres connection the database closed is made again", { skip: !url }, async (t) => {
  t.mock.method(console, "error", () => {});
  const db = await postgres({ url, ...within }, pg);
  const admin = await postgres({ url, ...within }, pg);
  const [[[pid]]] = (await db.transaction([said("SELECT pg_backend_pid()")])).map((answer) => answer.rows);
  await admin.transaction([said("SELECT count(pg_terminate_backend($1))", [pid])]);
  let answered;
  for (let tries = 0; tries < 100 && !answered; tries += 1) {
    answered = await db.transaction([said("SELECT pg_backend_pid()")]).catch(() => undefined);
    if (!answered) await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.ok(answered, "the closed connection was never made again");
  assert.notEqual(answered[0].rows[0][0], pid);
});
