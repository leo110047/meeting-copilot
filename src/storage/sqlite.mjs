import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

export function migrate(dbPath, schemaPath = new URL("./schema.sql", import.meta.url)) {
  const absoluteDb = resolve(dbPath);
  mkdirSync(dirname(absoluteDb), { recursive: true });
  const schema = readFileSync(schemaPath, "utf8");
  const result = spawnSync("sqlite3", [absoluteDb], { input: schema, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr || `sqlite3 exited with ${result.status}`);
  }
  return absoluteDb;
}

export function listTables(dbPath) {
  const result = spawnSync("sqlite3", [resolve(dbPath), ".tables"], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr);
  return result.stdout.trim().split(/\s+/).filter(Boolean);
}

export function executeSql(dbPath, sql) {
  const result = spawnSync("sqlite3", [resolve(dbPath)], { input: sql, encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr || `sqlite3 exited with ${result.status}`);
  return result.stdout;
}

export function queryScalar(dbPath, sql) {
  return executeSql(dbPath, sql).trim();
}

export function sqlString(value) {
  if (value === undefined || value === null) return "NULL";
  return `'${String(value).replaceAll("'", "''")}'`;
}

export function sqlNumber(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return "NULL";
  return String(Number(value));
}
