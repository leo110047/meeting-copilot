#!/usr/bin/env node
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

const root = fileURLToPath(new URL("..", import.meta.url));
const dist = resolve(root, "dist");
rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });
cpSync(resolve(root, "src/ui"), dist, { recursive: true });

const fixtureIndex = JSON.parse(readFileSync(resolve(root, "fixtures/index.json"), "utf8"));
for (const fixture of fixtureIndex.fixtures) {
  const content = JSON.parse(readFileSync(resolve(root, `fixtures/${fixture}.json`), "utf8"));
  for (const key of ["brief", "transcriptEvents", "expectedInterventionMoments", "baselineChecklist"]) {
    if (!(key in content)) throw new Error(`${fixture} missing ${key}`);
  }
}

writeFileSync(resolve(dist, "build-info.json"), JSON.stringify({
  builtAt: new Date().toISOString(),
  fixtures: fixtureIndex.fixtures
}, null, 2));

console.log(`Built shared frontend and validated ${fixtureIndex.fixtures.length} fixtures.`);
