#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionType {
    Scope,
    Priority,
    Commitment,
    Tradeoff,
    Owner,
    Timeline,
    Budget,
    TechnicalDirection,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingInputKind {
    Owner,
    Deadline,
    AcceptanceCriteria,
    Budget,
    DecisionMaker,
    RollbackPlan,
    SuccessMetric,
    Constraint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MissingInput {
    pub kind: MissingInputKind,
    pub text: String,
    pub blocks_decision: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionReadiness {
    pub score: f32,
    pub safe_to_decide: bool,
    pub blockers: Vec<String>,
    pub evidence_transcript_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionState {
    pub session_id: String,
    pub current_decision: Option<String>,
    pub decision_type: DecisionType,
    pub missing_inputs: Vec<MissingInput>,
    pub readiness: DecisionReadiness,
    pub evidence_transcript_ids: Vec<String>,
}

pub fn stable_canonical_key(
    kind: &str,
    semantic_label: &str,
    linked_playbook_item_id: Option<&str>,
) -> String {
    let mut normalized = semantic_label
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_");
    normalized.retain(|ch| {
        ch.is_ascii_alphanumeric() || ch == '_' || ('\u{4e00}'..='\u{9fff}').contains(&ch)
    });

    match linked_playbook_item_id {
        Some(linked) if !linked.is_empty() => format!("{kind}:{normalized}:{linked}"),
        _ => format!("{kind}:{normalized}"),
    }
}

pub fn evaluate_readiness(state: &DecisionState) -> DecisionReadiness {
    let blockers = state
        .missing_inputs
        .iter()
        .filter(|item| item.blocks_decision)
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();

    let score = if state.current_decision.is_none() {
        0.0
    } else {
        (1.0 - blockers.len() as f32 * 0.22).clamp(0.0, 1.0)
    };

    DecisionReadiness {
        score,
        safe_to_decide: score >= 0.72 && blockers.is_empty(),
        blockers,
        evidence_transcript_ids: state.evidence_transcript_ids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_key_is_deterministic() {
        let a = stable_canonical_key("risk", " Owner 還沒定  ", Some("must-owner"));
        let b = stable_canonical_key("risk", "owner 還沒定", Some("must-owner"));
        assert_eq!(a, b);
    }

    #[test]
    fn readiness_blocks_missing_owner_and_deadline() {
        let state = DecisionState {
            session_id: "s1".to_string(),
            current_decision: Some("v1 scope".to_string()),
            decision_type: DecisionType::Scope,
            missing_inputs: vec![
                MissingInput {
                    kind: MissingInputKind::Owner,
                    text: "owner missing".to_string(),
                    blocks_decision: true,
                },
                MissingInput {
                    kind: MissingInputKind::Deadline,
                    text: "deadline missing".to_string(),
                    blocks_decision: true,
                },
            ],
            readiness: DecisionReadiness {
                score: 1.0,
                safe_to_decide: true,
                blockers: vec![],
                evidence_transcript_ids: vec![],
            },
            evidence_transcript_ids: vec!["t1".to_string()],
        };

        let readiness = evaluate_readiness(&state);
        assert!(!readiness.safe_to_decide);
        assert_eq!(readiness.blockers.len(), 2);
    }
}
