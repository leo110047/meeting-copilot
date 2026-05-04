// Fallback heuristic for offline/local operation. Subscription OAuth extraction
// is the authoritative Layer 3 path when AI is enabled.
fn derive_decision_state(session_id: &str, events: &[TranscriptEvent]) -> NativeDecisionState {
    let texts: Vec<String> = events.iter().map(|event| event.text.clone()).collect();
    let joined = texts.join(" ").to_lowercase();
    let mut blockers = vec![];
    let mut missing_inputs = vec![];
    if contains_any(&joined, &["owner", "負責", "誰"])
        && contains_any(&joined, &["沒", "未", "還", "不清楚"])
    {
        blockers.push("還沒有明確 owner".to_string());
        missing_inputs.push(
            serde_json::json!({"kind":"owner","text":"還沒有明確 owner","blocksDecision":true}),
        );
    }
    if contains_any(&joined, &["deadline", "時程", "什麼時候"])
        && contains_any(&joined, &["沒", "未", "還", "不要"])
    {
        blockers.push("deadline 還沒有明確承諾".to_string());
        missing_inputs.push(serde_json::json!({"kind":"deadline","text":"deadline 還沒有明確承諾","blocksDecision":true}));
    }
    if contains_any(&joined, &["驗收", "acceptance", "成功標準"])
        && contains_any(&joined, &["沒", "未", "還", "不清楚"])
    {
        blockers.push("驗收標準還沒定".to_string());
        missing_inputs.push(serde_json::json!({"kind":"acceptance_criteria","text":"驗收標準還沒定","blocksDecision":true}));
    }
    let has_decision = contains_any(&joined, &["決定", "commit", "先這樣", "scope", "v1"]);
    let score = if has_decision {
        (1.0_f64 - blockers.len() as f64 * 0.22).max(0.0)
    } else {
        0.0
    };
    NativeDecisionState {
        session_id: session_id.to_string(),
        current_decision: events.last().map(|event| event.text.clone()),
        decision_type: if joined.contains("scope") || joined.contains("範圍") {
            "scope".to_string()
        } else {
            "unknown".to_string()
        },
        meeting_items: vec![],
        options: vec![],
        risks: vec![],
        missing_inputs,
        readiness: NativeReadiness {
            score,
            safe_to_decide: has_decision && blockers.is_empty() && score >= 0.72,
            blockers,
            evidence_transcript_ids: events.iter().map(|event| event.id.clone()).collect(),
        },
        evidence_transcript_ids: events.iter().map(|event| event.id.clone()).collect(),
    }
}

fn apply_live_state_patch(
    mut state: NativeDecisionState,
    patch: &LiveStatePatchEnvelope,
    events: &[TranscriptEvent],
) -> NativeDecisionState {
    let allowed_ids: HashSet<String> = events.iter().map(|event| event.id.clone()).collect();
    for item in &patch.meeting_state_patch.add_items {
        push_unique_state_value(&mut state.meeting_items, item, 220);
    }
    for item in &patch.meeting_state_patch.update_items {
        push_unique_state_value(&mut state.meeting_items, item, 220);
    }
    if let Some(phase) = patch
        .meeting_state_patch
        .phase_change
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        push_unique_state_value(
            &mut state.meeting_items,
            &serde_json::json!({"kind": "phase", "text": format!("會議階段：{phase}")}),
            220,
        );
    }
    for option in &patch.decision_state_patch.add_options {
        push_unique_state_value(&mut state.options, option, 180);
    }
    for option in &patch.decision_state_patch.update_options {
        push_unique_state_value(&mut state.options, option, 180);
    }
    let meeting_evidence = sanitize_evidence(
        &patch.meeting_state_patch.evidence_transcript_ids,
        &allowed_ids,
    );
    if !meeting_evidence.is_empty() {
        state.evidence_transcript_ids =
            merge_strings(&state.evidence_transcript_ids, &meeting_evidence);
        state.readiness.evidence_transcript_ids =
            merge_strings(&state.readiness.evidence_transcript_ids, &meeting_evidence);
    }
    let patch_evidence = sanitize_evidence(
        &patch.decision_state_patch.evidence_transcript_ids,
        &allowed_ids,
    );
    if !patch_evidence.is_empty() {
        state.evidence_transcript_ids =
            merge_strings(&state.evidence_transcript_ids, &patch_evidence);
        state.readiness.evidence_transcript_ids =
            merge_strings(&state.readiness.evidence_transcript_ids, &patch_evidence);
    }

    if let Some(current_decision) = patch
        .decision_state_patch
        .current_decision
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        state.current_decision = Some(current_decision.chars().take(180).collect());
    }

    for item in &patch.decision_state_patch.add_missing_inputs {
        let text = value_string(item, "text")
            .chars()
            .take(160)
            .collect::<String>();
        if text.is_empty()
            || state
                .missing_inputs
                .iter()
                .any(|existing| value_string(existing, "text") == text)
        {
            continue;
        }
        let kind = value_string(item, "kind");
        let blocks_decision = item
            .get("blocksDecision")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        state.missing_inputs.push(
            serde_json::json!({"kind": kind, "text": text, "blocksDecision": blocks_decision}),
        );
        if blocks_decision && !state.readiness.blockers.contains(&text) {
            state.readiness.blockers.push(text);
        }
    }

    for risk in &patch.decision_state_patch.add_risks {
        let text = value_string(risk, "text")
            .chars()
            .take(160)
            .collect::<String>();
        if text.is_empty() {
            continue;
        }
        let severity = value_string(risk, "severity");
        push_unique_state_value(
            &mut state.risks,
            &serde_json::json!({"severity": severity.clone(), "text": text}),
            160,
        );
        if matches!(severity.as_str(), "medium" | "high") {
            let blocker = format!("風險：{text}");
            if !state.readiness.blockers.contains(&blocker) {
                state.readiness.blockers.push(blocker);
            }
        }
    }

    if let Some(readiness) = &patch.decision_state_patch.readiness_patch {
        if let Some(score) = readiness.score {
            state.readiness.score = score.clamp(0.0, 1.0);
        }
        if let Some(blockers) = &readiness.blockers {
            for blocker in blockers
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                let blocker = blocker.chars().take(160).collect::<String>();
                if !state.readiness.blockers.contains(&blocker) {
                    state.readiness.blockers.push(blocker);
                }
            }
        }
        if let Some(evidence) = &readiness.evidence_transcript_ids {
            let evidence = sanitize_evidence(evidence, &allowed_ids);
            state.readiness.evidence_transcript_ids =
                merge_strings(&state.readiness.evidence_transcript_ids, &evidence);
        }
        if let Some(safe_to_decide) = readiness.safe_to_decide {
            state.readiness.safe_to_decide = safe_to_decide
                && state.readiness.blockers.is_empty()
                && state.readiness.score >= 0.72;
        }
    }

    if !state.readiness.blockers.is_empty() {
        state.readiness.safe_to_decide = false;
        state.readiness.score = state.readiness.score.min(0.68);
    }
    state
}

fn push_unique_state_value(
    target: &mut Vec<serde_json::Value>,
    value: &serde_json::Value,
    max_chars: usize,
) {
    let text = value_string(value, "text")
        .chars()
        .take(max_chars)
        .collect::<String>();
    if text.trim().is_empty() {
        return;
    }
    if target
        .iter()
        .any(|existing| value_string(existing, "text") == text)
    {
        return;
    }
    let mut normalized = value.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.insert("text".to_string(), serde_json::Value::String(text));
    } else {
        normalized = serde_json::json!({ "text": text });
    }
    target.push(normalized);
}

fn sanitize_evidence(values: &[String], allowed_ids: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|id| allowed_ids.contains(*id))
        .cloned()
        .collect()
}

fn merge_strings(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = vec![];
    for item in left.iter().chain(right.iter()) {
        if !merged.contains(item) {
            merged.push(item.clone());
        }
    }
    merged
}

fn value_string(value: &serde_json::Value, field: &str) -> String {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn derive_suggestions(
    _brief: &MeetingBrief,
    events: &[TranscriptEvent],
    decision_state: &NativeDecisionState,
) -> Vec<NativeSuggestion> {
    if decision_state.current_decision.is_none()
        || decision_state.readiness.safe_to_decide
        || decision_state.readiness.blockers.is_empty()
    {
        return vec![];
    }
    let text = "先不要定案，這裡還缺 owner、deadline 或驗收標準。建議先補問清楚再承諾 scope。";
    let evidence: Vec<String> = events.iter().map(|event| event.id.clone()).collect();
    vec![NativeSuggestion {
        id: stable_id(&format!(
            "identify_missing_input:{}:{}",
            decision_state.session_id,
            evidence.join(",")
        )),
        session_id: decision_state.session_id.clone(),
        shown_at: now_ms_string(),
        kind: "identify_missing_input".to_string(),
        text: text.to_string(),
        reason: format!(
            "Decision readiness score {:.2}; blockers: {}",
            decision_state.readiness.score,
            decision_state.readiness.blockers.join(", ")
        ),
        confidence: 0.86,
        priority: "high".to_string(),
        evidence_transcript_ids: evidence,
    }]
}

fn default_brief() -> MeetingBrief {
    MeetingBrief {
        session_id: format!("native_{}", now_ms()),
        project_id: Some("native_default_project".to_string()),
        meeting_type: "requirement_scoping".to_string(),
        title: Some("即時會議".to_string()),
        goal: "即時監聽會議決策，避免在 owner、deadline、驗收標準不清楚時承諾 scope".to_string(),
        must_confirm: vec![
            "owner".to_string(),
            "deadline".to_string(),
            "驗收標準".to_string(),
            "rollback plan".to_string(),
        ],
        risks: vec![
            "未定義 owner/deadline 就做承諾".to_string(),
            "demo scope 和正式版 scope 混在一起".to_string(),
        ],
        constraints: vec!["先確認決策條件再承諾交付".to_string()],
        known_participants: vec![],
        preferred_tone: "direct".to_string(),
        started_at: now_ms_string(),
    }
}

fn detect_language(text: &str) -> String {
    let has_chinese = text
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    let has_english = text.chars().any(|ch| ch.is_ascii_alphabetic());
    match (has_chinese, has_english) {
        (true, true) => "mixed",
        (true, false) => "zh-TW",
        (false, true) => "en",
        _ => "unknown",
    }
    .to_string()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn now_ms_string() -> String {
    // Stable enough for local audit rows without adding a time crate.
    format!("{}", now_ms())
}

fn stable_id(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
