import assert from "node:assert/strict";
import test from "node:test";
import { chooseDogfoodTextProvider, connectorHealth, StaticJsonProvider, SubscriptionOAuthCommandProvider } from "../src/providers/modelProvider.mjs";

test("text decision subscription provider is separate from STT role", () => {
  const provider = new StaticJsonProvider({
    id: "openai-codex-oauth",
    kind: "subscription_oauth",
    roles: ["text_decision", "replay"],
    output: {}
  });
  const health = connectorHealth(provider);
  assert.equal(health.authenticated, true);
  assert.equal(provider.roles.includes("stt"), false);
});

test("dogfood text provider prefers healthy subscription OAuth connector", () => {
  const oauth = new SubscriptionOAuthCommandProvider({
    id: "openai-codex-oauth",
    command: "codex",
    authenticated: true,
    canRefreshToken: true
  });
  const api = new StaticJsonProvider({ id: "api-provider", kind: "api", roles: ["text_decision"], output: {} });
  const chosen = chooseDogfoodTextProvider([api, oauth]);

  assert.equal(chosen.id, "openai-codex-oauth");
  assert.equal(chosen.roles.includes("stt"), false);
});

test("dogfood text provider falls back when OAuth connector is not authenticated", () => {
  const oauth = new SubscriptionOAuthCommandProvider({
    id: "openai-codex-oauth",
    command: "codex",
    authenticated: false,
    canRefreshToken: false
  });
  const api = new StaticJsonProvider({ id: "api-provider", kind: "api", roles: ["text_decision"], output: {} });
  const chosen = chooseDogfoodTextProvider([oauth, api]);

  assert.equal(chosen.id, "api-provider");
});
