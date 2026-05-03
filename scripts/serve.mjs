#!/usr/bin/env node
import { createLiveApiServer } from "../src/server/liveApi.mjs";

const port = Number(process.env.PORT ?? process.argv[2] ?? 8767);
const server = createLiveApiServer({
  distRoot: "dist",
  dbPath: process.env.MEETING_COPILOT_DB ?? ".data/meeting-copilot.db"
});
server.listen(port, "127.0.0.1", () => {
  console.log(`Meeting Copilot UI: http://127.0.0.1:${port}`);
});
