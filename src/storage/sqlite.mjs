import Database from "better-sqlite3";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const openDatabases = new Map();

export function migrate(dbPath, schemaPath = new URL("./schema.sql", import.meta.url)) {
  const absoluteDb = resolve(dbPath);
  mkdirSync(dirname(absoluteDb), { recursive: true });
  const schema = readFileSync(schemaPath, "utf8");
  const db = database(absoluteDb);
  db.exec(schema);
  ensureColumn(db, "suggestions", "suggestion_json", "TEXT NOT NULL DEFAULT '{}'");
  return absoluteDb;
}

export function listTables(dbPath) {
  return database(dbPath)
    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
    .all()
    .map((row) => row.name);
}

export function executeSql(dbPath, sql, params = []) {
  const db = database(dbPath);
  if (!params.length) {
    db.exec(sql);
    return "";
  }
  db.prepare(sql).run(...params);
  return "";
}

export function queryScalar(dbPath, sql, params = []) {
  const row = database(dbPath).prepare(sql).get(...params);
  if (!row) return "";
  const [value] = Object.values(row);
  return value === null || value === undefined ? "" : String(value);
}

export function queryRows(dbPath, sql, params = []) {
  return database(dbPath).prepare(sql).all(...params);
}

export function runTransaction(dbPath, callback) {
  const db = database(dbPath);
  return db.transaction(() => callback(db))();
}

export function closeDatabase(dbPath) {
  const absoluteDb = resolve(dbPath);
  const db = openDatabases.get(absoluteDb);
  if (!db) return;
  db.close();
  openDatabases.delete(absoluteDb);
}

export function closeAllDatabases() {
  for (const [absoluteDb, db] of openDatabases.entries()) {
    db.close();
    openDatabases.delete(absoluteDb);
  }
}

export function sqlString(value) {
  if (value === undefined || value === null) return "NULL";
  return `'${String(value).replaceAll("'", "''")}'`;
}

export function sqlNumber(value) {
  if (value === undefined || value === null || Number.isNaN(Number(value))) return "NULL";
  return String(Number(value));
}

function database(dbPath) {
  const absoluteDb = resolve(dbPath);
  let db = openDatabases.get(absoluteDb);
  if (!db) {
    mkdirSync(dirname(absoluteDb), { recursive: true });
    db = new Database(absoluteDb);
    db.pragma("foreign_keys = ON");
    openDatabases.set(absoluteDb, db);
  }
  return db;
}

function ensureColumn(db, table, column, definition) {
  const columns = db.prepare(`PRAGMA table_info(${table})`).all().map((row) => row.name);
  if (columns.includes(column)) return;
  db.exec(`ALTER TABLE ${table} ADD COLUMN ${column} ${definition}`);
}
