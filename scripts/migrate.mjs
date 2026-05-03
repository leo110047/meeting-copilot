#!/usr/bin/env node
import { migrate, listTables } from "../src/storage/sqlite.mjs";

const dbPath = process.argv[2] ?? ".data/meeting-copilot.db";
const migrated = migrate(dbPath);
const tables = listTables(migrated);
console.log(JSON.stringify({ dbPath: migrated, tableCount: tables.length, tables }, null, 2));
