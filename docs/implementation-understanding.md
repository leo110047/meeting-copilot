# Implementation Understanding

## Product Goal

Meeting Copilot is a Layer 3 decision copilot. It is not a single-meeting summarizer. The core job is to notice decision moments, evaluate readiness, retrieve cross-session context, and suggest a concrete decision move before the meeting commits to a weak decision.

## Architecture Decisions

- Tauri shell plus shared frontend is the desktop direction for both macOS and Windows.
- Rust core exists as a shared deterministic domain crate.
- The runnable Phase 0/1 loop is dependency-light Node so replay, fixtures, schema migration, and manual transcript testing work before live audio.
- STT provider and text decision provider are separate interfaces.
- Text decision dogfood primary is `subscription_oauth`; API and local are fallback roles.
- StateExtractionEngine may be nondeterministic, but it can only output patches.
- MeetingStateReducer and DecisionStateReducer are pure deterministic functions.
- Shared artifacts are built from objective meeting outputs only and deny private copilot fields.

## Phase Order

1. Phase 0: Domain contracts, storage schema, event bus, mock transcript pipeline.
2. Phase 0.5: Tauri shell skeleton, shared frontend, macOS status item / Windows tray platform shell contract.
3. Phase 1: Manual transcript full Layer 3 loop.
4. Phase 1.5: Replay harness before any STT/live audio work.
5. Phase 2+: Provider bake-off, mic live dogfood, system audio, real meeting dogfood, optional Vision.

## Main Risks

- A quiet system can look good while missing high-severity moments, so golden tests track recall@high and false negatives.
- A generic checklist can beat a weak coach; replay reports baseline delta.
- State can regress if LLM rewrites whole state, so reducers only apply patches and enforce no-regression rules.
- ParticipantProfile and PoliticalSignal are sensitive. PoliticalSignal is hypothesis-only and not bound to people.
- Shared artifact export can leak strategy or feedback; the builder has denylist tests.
