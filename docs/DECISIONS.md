# Decisions

## Readiness Score Patch Semantics

Decision readiness scores may decrease when a later patch adds blockers, missing inputs, or weaker evidence. The reducer intentionally applies the latest readiness patch instead of forcing monotonic growth, because "more transcript" can reveal that a meeting is less ready to decide than the earlier local heuristic suggested.
