# Meeting Copilot UI Design

`docs/uiux-research.md` is the supporting research. This file is the implementation source of truth for the product UI.

## Product Interface Model

Meeting Copilot uses a three-stage desktop utility model:

1. `setup`: pre-flight context capture.
2. `listening`: transparent decision HUD.
3. `review`: document review and export.

The app is not a dashboard, chat app, or single-meeting note editor. It is a low-attention Layer 3 decision copilot. During a meeting, the UI must protect attention and show only decision-relevant signals.

## Global Rules

- Discussion and UI copy use Traditional Chinese.
- Code identifiers remain English.
- The interface must look like a native desktop utility, not a marketing page.
- Do not use decorative orbs, bokeh, or hero graphics.
- Avoid nested cards. Cards are allowed only for repeated documents, bounded panels, or intervention cards.
- Use progressive disclosure: show the current stage's essential controls, reveal secondary detail on demand.
- AI must be visibly enabled before live listening can start.
- Any AI suggestion must be dismissible, correctable, and evidence-backed.

## Stage Rules

### Setup

Purpose: answer "Are we ready, and what should the AI protect?"

Visible elements:

- AI readiness strip.
- Meeting context input with dictation button.
- File drop zone with count-only summary.
- AI pre-flight summary.
- Start button and disabled reason.

Do not show transcript, export, replay tools, or live suggestion controls.

### Listening

Purpose: answer "Is there a decision risk I should act on right now?"

Visible elements:

- Thin control bar: listening state, provider status, source, opacity, stop.
- Decision signal area.
- One intervention card when needed.
- Transcript drawer, collapsed by default.

No full dashboard during live meetings. The HUD must remain usable from 10% to 100% opacity.

### Review

Purpose: answer "What did this meeting produce?"

Visible elements:

- AI summary document.
- Transcript document.
- Primary export actions.
- Secondary export formats.
- New meeting action.

Primary exports:

- AI summary Markdown.
- Transcript TXT.

Secondary exports:

- JSON.
- PDF.

## Suggestion Card Anatomy

Every intervention card includes:

- Move type: Hold / Ask / Clarify / Decide.
- One sentence suggestion.
- Short reason.
- Evidence disclosure.
- Feedback actions: Useful, Too noisy, Wrong, Skip.

When the AI is uncertain, the card should ask a clarifying question instead of giving a directive.

## Visual System

- Base: graphite / charcoal.
- Material: translucent glass only during listening.
- Accent: one restrained decision signal color.
- Warning: amber/red only for real risk.
- Typography: system UI font for native feel.
- Border radius: 8px or less for controls; panels may use 10px max.
- Layout density: compact and scannable.

## Platform Shell

macOS status item and Windows tray are command surfaces only:

- Open Meeting Copilot.
- Start Listening.
- Stop Listening.
- New Meeting.
- Settings.
- Quit.

The tray/status item must not become the full product UI.
