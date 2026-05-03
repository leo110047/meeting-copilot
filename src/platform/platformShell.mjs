export const PLATFORM_KINDS = ["macos", "windows"];

export class PlatformShellContract {
  constructor(platform, capabilities) {
    if (!PLATFORM_KINDS.includes(platform)) throw new Error(`unsupported platform: ${platform}`);
    this.platform = platform;
    this.capabilities = capabilities;
  }

  async requestPermissions() {
    return {
      microphone: "unknown",
      systemAudio: "unknown",
      screenCapture: "unknown"
    };
  }

  async showSuggestion(move) {
    return {
      command: "show_suggestion",
      surface: this.capabilities.suggestionSurface,
      moveId: move.id,
      text: move.text
    };
  }

  async updateTrayState(state) {
    return {
      command: "update_status_entry",
      surface: this.capabilities.statusSurface,
      state
    };
  }

  async openReviewWindow(sessionId) {
    return {
      command: "open_review_window",
      surface: "window",
      sessionId
    };
  }
}

export class MacOSStatusItemShell extends PlatformShellContract {
  constructor() {
    super("macos", {
      statusSurface: "macos_status_item",
      suggestionSurface: "popover",
      systemAudioCapture: "screencapturekit",
      micCapture: "coreaudio",
      permissionModel: "macos_tcc"
    });
  }
}

export class WindowsTrayShell extends PlatformShellContract {
  constructor() {
    super("windows", {
      statusSurface: "windows_system_tray",
      suggestionSurface: "flyout",
      systemAudioCapture: "wasapi_loopback",
      micCapture: "wasapi_capture",
      permissionModel: "windows_privacy_settings"
    });
  }
}

export function detectPlatformShell(platform = process.platform) {
  if (platform === "darwin") return new MacOSStatusItemShell();
  if (platform === "win32") return new WindowsTrayShell();
  throw new Error(`unsupported host platform for desktop shell: ${platform}`);
}

export function createPlatformShell(platformKind) {
  if (platformKind === "macos") return new MacOSStatusItemShell();
  if (platformKind === "windows") return new WindowsTrayShell();
  throw new Error(`unsupported platform kind: ${platformKind}`);
}
