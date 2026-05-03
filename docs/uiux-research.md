# Meeting Copilot UI/UX Research

This document records the interface-pattern research for Meeting Copilot. It is a design input, not an implementation spec. The product goal is a Layer 3 decision copilot, so the UI must protect meeting attention while still making decision risk visible at the right moment.

## Sources Reviewed

- Apple Human Interface Guidelines: Menu bar, menu bar extras, app menus, windows, popovers, alerts, notifications, and machine learning.
  - https://developer.apple.com/design/human-interface-guidelines/the-menu-bar
  - https://developer.apple.com/design/human-interface-guidelines/windows
  - https://developer.apple.com/design/human-interface-guidelines/popovers
  - https://developer.apple.com/design/human-interface-guidelines/alerts
  - https://developer.apple.com/design/human-interface-guidelines/managing-notifications
  - https://developer.apple.com/design/human-interface-guidelines/machine-learning/
- Microsoft Windows / Fluent guidance: notification area, app notifications, flyouts, command bar flyouts, dialogs, menus.
  - https://learn.microsoft.com/en-us/windows/win32/shell/notification-area
  - https://learn.microsoft.com/en-us/windows/win32/uxguide/winenv-notification
  - https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-ux-guidance
  - https://learn.microsoft.com/en-us/windows/apps/design/controls/command-bar-flyout
  - https://learn.microsoft.com/en-us/windows/apps/design/controls/dialogs-and-flyouts/dialogs
  - https://learn.microsoft.com/en-us/windows/apps/design/controls/menus
- Human-AI interaction guidance:
  - Microsoft Research, Guidelines for Human-AI Interaction, CHI 2019.
    https://www.microsoft.com/en-us/research/publication/guidelines-for-human-ai-interaction/
  - Microsoft Research summary of the 18 Human-AI Interaction Guidelines.
    https://www.microsoft.com/en-us/research/articles/guidelines-for-human-ai-interaction-eighteen-best-practices-for-human-centered-ai-design/
- General UX pattern research:
  - Progressive Disclosure pattern.
    https://ui-patterns.com/patterns/ProgressiveDisclosure
  - Interaction Design Foundation, Progressive Disclosure.
    https://www.interaction-design.org/literature/topics/progressive-disclosure

## Product-Specific UX Premise

Meeting Copilot is not a meeting note editor, dashboard, or chat app. It is a low-attention decision-risk assistant. The primary user is already doing another task: listening, talking, watching a shared screen, and deciding whether to push back.

Therefore, the UI should optimize for:

- Low interruption during live meetings.
- Fast recognition of risk and missing inputs.
- Clear control over when AI listens and what it sends.
- Evidence-backed suggestions, not mysterious AI assertions.
- Strong separation between setup, live assistance, and post-meeting documents.

## Core Recommendation

Use a three-stage interface:

1. Setup: pre-flight context capture.
2. Listening: transparent decision HUD.
3. Review: document export and correction.

This is the healthiest design direction because it maps UI complexity to the user's current task. It follows progressive disclosure: only show what is useful at the current stage, reveal deeper detail only on demand.

### Assumptions

- Users open the app shortly before or during a meeting.
- Users need to see shared-screen content behind the app during live mode.
- AI must be explicitly enabled before meeting content is sent to the text provider.
- Transcript detail is useful, but not always worth occupying the primary surface.
- The product must behave like a desktop utility, not a web dashboard.

### Main Failure Modes

- The app shows too much information during the meeting and competes with the meeting.
- Suggestions appear without evidence or confidence and feel untrustworthy.
- Setup hides AI/privacy state too deeply, causing accidental data sharing concerns.
- Tray/status entry becomes a second full app UI and conflicts with OS conventions.
- The live HUD becomes too transparent to recover or too opaque to watch shared screens.

### Early Warning Signals

- User reads the app instead of participating in the meeting.
- User cannot explain why a suggestion appeared.
- User dismisses most suggestions as noise.
- User cannot find Stop, opacity, or transcript when needed.
- User asks whether AI is actually connected or what content is being sent.

### Best Alternative

A conventional dashboard with live transcript, state cards, suggestions, and export controls visible at once is easier to implement. It becomes better only if the app is used mostly after meetings for analysis. It is worse for real-time decision support because it increases attention cost during the meeting.

### Unknowns To Verify

- Whether users prefer the live suggestion surface centered, top-right, or bottom-right.
- Whether transcript drawer should open from bottom or side on Windows.
- How often users want proactive suggestions versus manual "ask copilot now".
- Whether opacity should be per-meeting remembered or global.
- Whether post-meeting PDF export should be native print, generated PDF, or both.

## Interface Pattern Mapping

| Need | Recommended Pattern | Why | Avoid |
| --- | --- | --- | --- |
| Start/stop app from desktop shell | macOS menu bar extra / Windows notification area icon | Native access point for background utilities | Large custom tray popup as the main product |
| Pre-meeting context | Pre-flight panel | User needs to confirm readiness before listening | Multi-step wizard unless setup grows complex |
| AI readiness | Status badge + blocking primary action | Start must be disabled until AI is ready | Hidden settings page as the only AI control |
| File input | Drop zone with count-only summary | Files are context, not content to inspect live | Large file preview list |
| AI prep summary | Review block | Confirms what the AI understood before starting | Silent background ingestion |
| Live assistance | Transparent HUD | Keeps meeting/shared screen visible | Dashboard layout |
| Transcript | Collapsible drawer | Useful secondary detail on demand | Always-expanded transcript panel |
| Intervention | Alert card inside HUD | Strong focus only when a decision risk appears | Toasts for every observation |
| Evidence | Expandable evidence row | Trust without clutter | Long reasoning visible by default |
| Feedback | Inline granular feedback buttons | Supports AI correction and policy tuning | Generic thumbs up/down only |
| End meeting | Explicit transition to review | Meeting produces documents | Direct quit/close |
| Export | Document review + export panel | User is now in document mode | Mixing download controls into live HUD |
| Errors | Inline status for recoverable issues, modal only for blockers | Avoid needless interruption | Alerts for common transient errors |

## Stage 1: Setup / Pre-Flight

Goal: answer "Are we ready to let AI listen, and what should it protect?"

Recommended layout:

- Top compact AI readiness strip.
  - States: Not signed in, signed in but not enabled, enabled, provider error.
  - Primary actions: Login, Enable AI.
  - Always state whether meeting content will be sent.
- Main context input.
  - One textarea for typed notes.
  - Voice dictation action attached to the same context area.
  - Placeholder should be action-oriented: what decision, what constraints, what must not be promised.
- File drop zone.
  - Count-only display.
  - Per-file errors summarized inline.
  - No full preview by default.
- AI pre-flight summary.
  - "Today protect" bullets.
  - "Must ask before committing" bullets.
  - "Known risks" bullets.
- Start button.
  - Disabled until AI is authenticated and explicitly enabled.
  - Disabled reason visible next to it.

Do not use:

- Wizard: setup is one decision, not multiple sequential forms.
- Chat UI: the user is not trying to converse with the app before every meeting.
- Dashboard cards: they visually overstate secondary context.

## Stage 2: Listening / Transparent HUD

Goal: answer "Is there a decision risk I should act on right now?"

Recommended layout:

- Top control bar:
  - Listening status.
  - STT source.
  - AI provider state.
  - Opacity slider, 10-100%.
  - Stop button.
- Center signal area:
  - Empty state: very small, low-contrast "listening, no intervention".
  - Decision state: current decision, readiness, top blocker.
  - Intervention: one prominent suggestion card.
- Transcript drawer:
  - Closed by default.
  - Shows latest 1-3 lines when closed.
  - Expanded view shows speaker/source labels and recent transcript.
- Optional evidence drawer inside suggestion:
  - Shows transcript snippets used by the suggestion.
  - Must be user-invoked, not always visible.

Intervention card anatomy:

- Move type: Hold / Ask / Clarify / Decide.
- One sentence suggestion.
- Reason: one concise clause.
- Evidence: "based on N lines" expandable.
- Confidence/readiness: lightweight status, not a fake precise score unless needed.
- Feedback: Useful, Too noisy, Wrong, Skip.

Do not use:

- Full-screen dashboard.
- Persistent large transcript panel.
- Toasts for live decision suggestions; toasts can disappear too fast and lack evidence.
- Modal during a live meeting unless recording/privacy permission blocks the core flow.

## Stage 3: Review / Document Exit

Goal: answer "What did the meeting produce, and what should I keep/share?"

Recommended layout:

- Header:
  - Meeting ended.
  - AI summary provider status.
  - New meeting action.
- Two document columns:
  - AI Summary.
  - Transcript.
- Primary export buttons:
  - Summary Markdown.
  - Transcript TXT.
- Secondary export menu:
  - JSON.
  - PDF.
- Correction affordances:
  - Edit title.
  - Mark suggestion as wrong/noisy after the fact.
  - Regenerate AI summary if provider was unavailable or transcript changed.

Do not use:

- Six equally weighted download buttons.
- Hiding transcript export behind AI summary.
- Treating JSON as the default user-facing artifact.

## Desktop Shell Pattern

### macOS

Apple guidance treats menu bar extras as lightweight status/command access. A click should usually show a menu unless the functionality requires more complexity. For Meeting Copilot:

- Menu bar extra menu:
  - Open Meeting Copilot.
  - Start Listening if ready.
  - Stop Listening if active.
  - New Meeting.
  - Settings.
  - Quit.
- The full setup/listening/review UI should be an app window or detachable auxiliary panel, not a complex menu bar popover.

### Windows

Windows notification area is a status/notification access point. It should not become noisy or promotional. For Meeting Copilot:

- Tray icon:
  - Show app.
  - Start.
  - Stop.
  - New meeting.
  - Quit.
- Flyout/panel:
  - Short state and primary commands only.
- Full review should open in a normal window.

## AI UX Rules

Apply Microsoft Human-AI Interaction Guidelines directly:

- Make clear what AI can do.
  - Setup must say: prep summary, live decision patch, post-meeting summary.
- Make clear how well it can do it.
  - Use "may be wrong" and show evidence/confidence when suggestions appear.
- Time services based on context.
  - Do not interrupt for weak signals.
  - Escalate only for decision risk, missing owner/deadline/acceptance, conflict with context, or unsafe commitment.
- Show contextually relevant information.
  - Live HUD shows only the current decision state, not general notes.
- Support efficient invocation.
  - Add a later "Ask now" or "Analyze latest" action if users want manual pull.
- Support efficient dismissal.
  - Every suggestion must be dismissible with one action.
- Support efficient correction.
  - "Wrong" feedback should attach to the specific suggestion and evidence.
- Scope services when in doubt.
  - If confidence is low, ask a clarifying question instead of giving a directive.
- Make clear why the system acted.
  - Every intervention needs a short reason and expandable evidence.
- Provide global controls.
  - Settings must include AI enablement, data sending disclosure, intervention sensitivity, and transcript capture source.

## Notification / Interruption Policy

Use interruption levels:

1. Passive state: no card, only small status.
2. Low priority: update decision overview silently.
3. Medium priority: show suggestion card without sound.
4. High priority: visually emphasize card; still no modal.
5. Blocking: modal only for permission/privacy/provider failures that prevent the user-requested action.

Never interrupt for:

- General summary updates.
- Transcript line arrival.
- Low-confidence speculation.
- Repeated versions of the same blocker.

## Visual Direction

The product should feel like a native desktop utility for serious meetings:

- Use system typography.
- Avoid marketing-style hero sections.
- Avoid decorative gradient/orb backgrounds.
- Avoid heavy card grids.
- Use restrained neutral surfaces with one strong intervention color.
- Prefer compact controls and stable dimensions.
- The live HUD should be translucent, not visually dense.
- The setup and review screens can be more opaque because the user is not watching shared screen content.

Recommended palette direction:

- Base: near-black / graphite with transparent material.
- Text: high-contrast off-white.
- Muted: neutral gray.
- Accent: one decision-signal color.
- Warning: amber or red only for real risk.
- AI ready: quiet green/blue status, not neon.

## Recommended Information Architecture

```text
App shell
  Setup
    AI readiness
    Context input
    File drop
    AI pre-flight summary
    Start
  Listening HUD
    Control bar
    Decision signal
    Suggestion intervention
    Transcript drawer
  Review
    AI summary document
    Transcript document
    Export actions
    Feedback/correction
  Settings
    AI provider
    STT provider
    Privacy disclosure
    Intervention sensitivity
    Opacity default
```

## Implementation Priority

1. Create `DESIGN.md` from this research and make it the UI source of truth.
2. Refactor setup screen into a true pre-flight layout.
3. Refactor listening screen into a transparent HUD with no dashboard cards.
4. Add suggestion-card anatomy: move type, reason, evidence, feedback.
5. Make transcript drawer visually secondary.
6. Refactor review screen into two documents with primary/secondary exports.
7. Add settings window for AI/STT/privacy/intervention sensitivity.
8. Run screenshot verification on macOS and Windows-sized viewports.

## Design Decision

Use pre-flight + transparent HUD + document review as the core operating model.

Why it fits now:

- It matches the product's Layer 3 goal.
- It respects platform shell conventions.
- It minimizes live-meeting attention cost.
- It supports AI trust through evidence and feedback.

What would change this decision:

- If user testing shows people mostly use the app after meetings, move toward a review/dashboard model.
- If live AI becomes highly accurate and low-latency, add manual invocation and more subtle proactive signals.
- If screen sharing visibility becomes less important, opacity can become a secondary setting rather than a first-class control.
