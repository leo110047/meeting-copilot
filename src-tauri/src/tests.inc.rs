#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_context_file_reads_utf8_text() {
        let path = std::env::temp_dir().join(format!("meeting-copilot-drop-{}.md", now_ms()));
        fs::write(&path, "今天只確認 demo scope").expect("write temp context file");
        let file = read_dropped_context_file(path.clone());
        let _ = fs::remove_file(path);
        assert!(file.name.starts_with("meeting-copilot-drop-"));
        assert!(file.text.contains("demo scope"));
        assert!(!file.truncated);
        assert!(file.error.is_none());
    }

    #[test]
    fn dropped_context_file_rejects_unsupported_extension() {
        let path = std::env::temp_dir().join(format!("meeting-copilot-drop-{}.png", now_ms()));
        fs::write(&path, "not actually an image").expect("write temp unsupported file");
        let file = read_dropped_context_file(path.clone());
        let _ = fs::remove_file(path);
        assert!(file.text.is_empty());
        assert!(file.error.unwrap_or_default().contains("只支援文字檔"));
    }

    #[test]
    fn oauth_status_parser_does_not_accept_negative_logged_in_text() {
        assert_eq!(
            parse_subscription_oauth_authenticated("Not logged in. Run codex login to use ChatGPT."),
            SubscriptionOAuthParse::Unauthenticated
        );
        assert_eq!(
            parse_subscription_oauth_authenticated("ChatGPT login required"),
            SubscriptionOAuthParse::Unauthenticated
        );
        assert_eq!(
            parse_subscription_oauth_authenticated("Logged in using ChatGPT"),
            SubscriptionOAuthParse::Authenticated
        );
        assert_eq!(
            parse_subscription_oauth_authenticated("Codex CLI status changed its wording"),
            SubscriptionOAuthParse::Unknown
        );
    }

    #[test]
    fn transcript_cleanup_parser_accepts_text_only_shape() {
        let cleaned =
            parse_transcript_cleanup_text(r#"{"text":"這場只確認 demo 範圍，不承諾正式版時程。"}"#)
                .expect("parse cleanup text");
        assert_eq!(cleaned, "這場只確認 demo 範圍，不承諾正式版時程。");
    }

    #[test]
    fn transcript_cleanup_prompt_preserves_meaning_boundary() {
        let prompt = build_transcript_cleanup_prompt(&TranscriptCleanupRequest {
            text: "呃我們就是先確認 demo scope 然後不承諾 deadline".to_string(),
            context: "test".to_string(),
        })
        .expect("build cleanup prompt");
        assert!(prompt.contains("Do not summarize"));
        assert!(prompt.contains("Preserve the original meaning"));
        assert!(prompt.contains("names, numbers, dates, owners, deadlines, scope"));
    }

    #[test]
    fn app_error_log_round_trips_diagnostic_detail() {
        let db_path =
            std::env::temp_dir().join(format!("meeting-copilot-error-log-{}.db", now_ms()));
        let conn = open_db(&db_path).expect("open temp db");
        let record = AppErrorLogRecord {
            id: "error-log-test".to_string(),
            session_id: Some("session-test".to_string()),
            stage: "native_transcription.stderr".to_string(),
            source: "native_speech_helper".to_string(),
            severity: "error".to_string(),
            message: "helper failed".to_string(),
            detail_json: serde_json::json!({"platform": "test"}),
            created_at: now_ms_string(),
        };
        insert_app_error_log(&conn, &record).expect("insert app error log");
        let records =
            list_app_error_logs(&conn, Some("session-test")).expect("list app error logs");
        let _ = fs::remove_file(db_path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].stage, "native_transcription.stderr");
        assert_eq!(records[0].detail_json["platform"], "test");
    }

    #[test]
    fn dropped_context_requires_native_drop_grant() {
        let path = std::env::temp_dir().join(format!("meeting-copilot-ungranted-{}.md", now_ms()));
        fs::write(&path, "private meeting notes").expect("write temp context file");
        let file = read_granted_dropped_context_file(path.clone());
        assert!(file.text.is_empty());
        assert!(file.error.unwrap_or_default().contains("未經本次拖拉授權"));

        register_drop_read_grants(&[path.clone()]);
        let granted = read_granted_dropped_context_file(path.clone());
        let _ = fs::remove_file(path);
        assert!(granted.error.is_none());
        assert!(granted.text.contains("private meeting notes"));
    }

    #[test]
    fn live_patch_applies_meeting_items_options_and_risks() {
        let events = vec![TranscriptEvent {
            id: "e1".to_string(),
            session_id: "s1".to_string(),
            source: "mic".to_string(),
            speaker: Some("A".to_string()),
            speaker_confidence: 0.7,
            language: "mixed".to_string(),
            started_at_ms: 0,
            ended_at_ms: Some(1),
            text: "先比較方案 A 和 B，但風險還沒釐清".to_string(),
            is_final: true,
        }];
        let state = derive_decision_state("s1", &events);
        let patch = LiveStatePatchEnvelope {
            meeting_state_patch: LiveMeetingStatePatch {
                add_items: vec![
                    serde_json::json!({"kind": "context", "text": "客戶只要 demo scope"}),
                ],
                update_items: vec![],
                resolve_item_ids: vec![],
                phase_change: Some("對齊方案".to_string()),
                evidence_transcript_ids: vec!["e1".to_string()],
            },
            decision_state_patch: LiveDecisionStatePatch {
                current_decision: Some("先做 demo scope".to_string()),
                add_options: vec![serde_json::json!({"text": "方案 A"})],
                update_options: vec![],
                add_risks: vec![
                    serde_json::json!({"severity": "high", "text": "正式版 scope 被偷渡"}),
                ],
                add_missing_inputs: vec![],
                readiness_patch: None,
                evidence_transcript_ids: vec!["e1".to_string()],
            },
        };

        let patched = apply_live_state_patch(state, &patch, &events);
        assert!(
            patched
                .meeting_items
                .iter()
                .any(|item| value_string(item, "text").contains("demo scope"))
        );
        assert!(
            patched
                .options
                .iter()
                .any(|item| value_string(item, "text") == "方案 A")
        );
        assert!(
            patched
                .risks
                .iter()
                .any(|item| value_string(item, "text").contains("偷渡"))
        );
        assert!(
            patched
                .readiness
                .blockers
                .iter()
                .any(|item| item.contains("風險"))
        );
    }

    #[test]
    fn live_patch_rejects_nested_full_rewrite_attempt() {
        let raw = serde_json::json!({
            "meetingStatePatch": {
                "addItems": [{"text": "bad", "decisionState": {"currentDecision": "rewrite"}}],
                "updateItems": [],
                "resolveItemIds": [],
                "evidenceTranscriptIds": []
            },
            "decisionStatePatch": {
                "addOptions": [],
                "updateOptions": [],
                "addRisks": [],
                "addMissingInputs": [],
                "evidenceTranscriptIds": []
            }
        });
        assert!(
            validate_live_state_patch_value(&raw)
                .unwrap_err()
                .contains("nested full rewrite")
        );
    }
}
