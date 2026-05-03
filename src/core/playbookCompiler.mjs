import { sha16 } from "../domain/contracts.mjs";

const FIELD_PRIORITY = {
  mustConfirm: "high",
  risks: "high",
  constraints: "medium",
  goal: "medium"
};

export class PlaybookCompiler {
  compile(brief) {
    const mustConfirm = (brief.mustConfirm ?? []).slice(0, 6).map((label, index) => item("must", label, index));
    const riskWatch = (brief.risks ?? []).slice(0, 6).map((label, index) => item("risk", label, index));
    const interventionRules = [];

    for (const entry of mustConfirm) {
      interventionRules.push(rule("mustConfirm", entry.label, `先確認：${entry.label}`, FIELD_PRIORITY.mustConfirm));
    }
    for (const entry of riskWatch) {
      interventionRules.push(rule("risks", entry.label, `把風險攤開：${entry.label}`, FIELD_PRIORITY.risks));
    }
    for (const constraint of brief.constraints ?? []) {
      interventionRules.push(rule("constraints", constraint, `確認限制是否仍成立：${constraint}`, FIELD_PRIORITY.constraints));
    }

    return {
      id: sha16(`playbook:${brief.sessionId}:${brief.goal}`),
      meetingType: brief.meetingType,
      objective: brief.goal,
      mustConfirm,
      riskWatch,
      interventionRules: interventionRules.slice(0, 8),
      layer3Hooks: [
        { kind: "project_history", query: brief.goal, requiredForMvp: false },
        { kind: "participant_pattern", query: (brief.knownParticipants ?? []).map((p) => p.name).join(" "), requiredForMvp: false },
        { kind: "strategic_context", query: brief.goal, requiredForMvp: false }
      ]
    };
  }
}

function item(prefix, label, index) {
  return {
    id: sha16(`${prefix}:${index}:${label}`),
    label,
    status: "unknown",
    evidenceTranscriptIds: []
  };
}

function rule(linkedBriefField, label, suggestedMove, priority) {
  return {
    id: sha16(`rule:${linkedBriefField}:${label}`),
    trigger: label,
    suggestedMove,
    priority,
    linkedBriefField
  };
}
