import { spawn } from "node:child_process";

export class StaticJsonProvider {
  constructor({ id = "local-rule-provider", kind = "local", roles = ["text_decision", "replay"], output }) {
    this.id = id;
    this.kind = kind;
    this.roles = roles;
    this.supportsStructuredOutput = true;
    this.supportsPromptCaching = false;
    this.supportsReplay = true;
    this.supportsUsageTelemetry = true;
    this.output = output;
  }

  async runStructured(request) {
    const started = Date.now();
    return {
      output: typeof this.output === "function" ? this.output(request.input) : this.output,
      usage: usage({ request, provider: this.id, model: this.kind, latencyMs: Date.now() - started })
    };
  }
}

export class SubscriptionOAuthCommandProvider {
  constructor({
    id = "openai-codex-oauth",
    command,
    args = [],
    authenticated = false,
    canRefreshToken = false,
    spawnProcess = spawn
  } = {}) {
    this.id = id;
    this.kind = "subscription_oauth";
    this.roles = ["text_decision", "memory_extraction"];
    this.supportsStructuredOutput = true;
    this.supportsPromptCaching = true;
    this.supportsReplay = false;
    this.supportsUsageTelemetry = false;
    this.supportsStreaming = true;
    this.authenticated = authenticated;
    this.canRefreshToken = canRefreshToken;
    this.command = command;
    this.args = args;
    this.spawnProcess = spawnProcess;
    this.lastError = undefined;
  }

  async runStructured(request) {
    if (!this.command) {
      this.lastError = "subscription OAuth command is not configured";
      throw new Error(this.lastError);
    }
    const output = await runCommandJson({
      command: this.command,
      args: this.args,
      input: JSON.stringify(request),
      spawnProcess: this.spawnProcess
    });
    return {
      output,
      usage: usage({ request, provider: this.id, model: this.kind, latencyMs: 0 })
    };
  }
}

export class BrokenProvider {
  constructor({ id, errorKind, output }) {
    this.id = id;
    this.kind = "api";
    this.roles = ["text_decision", "replay"];
    this.supportsStructuredOutput = false;
    this.supportsPromptCaching = false;
    this.supportsReplay = true;
    this.supportsUsageTelemetry = false;
    this.errorKind = errorKind;
    this.output = output;
  }

  async runStructured() {
    if (this.errorKind === "api_error") throw new Error("provider api error");
    if (this.errorKind === "timeout") {
      await new Promise((resolve) => setTimeout(resolve, 25));
      const err = new Error("provider timeout");
      err.code = "TIMEOUT";
      throw err;
    }
    return { output: this.output };
  }
}

export function chooseDogfoodTextProvider(providers) {
  const ranked = [
    (provider) => provider.kind === "subscription_oauth" && connectorHealth(provider).authenticated && connectorHealth(provider).supportsStructuredOutput,
    (provider) => provider.kind === "api" && connectorHealth(provider).supportsStructuredOutput,
    (provider) => provider.kind === "local" && connectorHealth(provider).supportsStructuredOutput
  ];
  for (const predicate of ranked) {
    const found = providers.find((provider) => provider.roles?.includes("text_decision") && predicate(provider));
    if (found) return found;
  }
  return undefined;
}

export function connectorHealth(provider) {
  return {
    providerId: provider.id,
    authenticated: provider.kind !== "subscription_oauth" || Boolean(provider.authenticated ?? true),
    canRefreshToken: provider.kind !== "subscription_oauth" || Boolean(provider.canRefreshToken ?? true),
    supportsStreaming: Boolean(provider.supportsStreaming ?? false),
    supportsStructuredOutput: Boolean(provider.supportsStructuredOutput),
    supportsReplay: Boolean(provider.supportsReplay),
    supportsUsageTelemetry: Boolean(provider.supportsUsageTelemetry),
    lastError: provider.lastError
  };
}

function runCommandJson({ command, args, input, spawnProcess }) {
  return new Promise((resolve, reject) => {
    const child = spawnProcess(command, args, { stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => stdout += String(chunk));
    child.stderr.on("data", (chunk) => stderr += String(chunk));
    child.on("error", reject);
    child.on("exit", (code) => {
      if (code !== 0) {
        reject(new Error(stderr || `${command} exited ${code}`));
        return;
      }
      try {
        resolve(JSON.parse(stdout));
      } catch (error) {
        reject(error);
      }
    });
    child.stdin.end(input);
  });
}

function usage({ request, provider, model, latencyMs }) {
  const serialized = JSON.stringify(request.input ?? {});
  return {
    id: `usage_${Date.now()}`,
    sessionId: request.input?.sessionId ?? "unknown",
    callType: request.callType,
    provider,
    model,
    promptVersion: request.promptVersion,
    promptHash: request.promptVersion,
    inputTokens: Math.ceil(serialized.length / 4),
    cachedInputTokens: 0,
    outputTokens: 64,
    latencyMs,
    createdAt: new Date().toISOString()
  };
}
