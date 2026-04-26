#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tokio::sync::{broadcast, Mutex};
    use tower::ServiceExt;

    use coast_core::protocol::CoastEvent;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    use crate::api;
    use crate::api::ws_host_terminal::PtySession;
    use crate::server::AppState;
    use crate::state::StateDb;

    fn test_app() -> axum::Router {
        let db = StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));
        api::api_router(state)
    }

    fn test_state() -> Arc<AppState> {
        let db = StateDb::open_in_memory().unwrap();
        Arc::new(AppState::new_for_testing(db))
    }

    fn test_state_with_docker() -> Arc<AppState> {
        let db = StateDb::open_in_memory().unwrap();
        Arc::new(AppState::new_for_testing_with_docker(db))
    }

    fn make_instance(project: &str, name: &str, build_id: Option<String>) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            status: InstanceStatus::Running,
            project: project.to_string(),
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("test-container".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id,
            coastfile_type: None,
            remote_host: None,
        }
    }

    fn write_manifest_with_agent_command(project: &str, build_id: &str, command: &str) {
        let home = dirs::home_dir().unwrap();
        let dir = home
            .join(".coast")
            .join("images")
            .join(project)
            .join(build_id);
        fs::create_dir_all(&dir).unwrap();
        let manifest = serde_json::json!({
            "agent_shell": {
                "command": command
            }
        });
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn remove_project_images_dir(project: &str) {
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".coast").join("images").join(project);
            let _ = fs::remove_dir_all(path);
        }
    }

    /// Write a flat `~/.coast/images/<project>/manifest.json` whose
    /// `project_root` field points at the given on-disk directory. The
    /// flat-manifest fallback in `builds_coastfile_types` makes this the
    /// minimal fixture for endpoint tests that don't care about a full
    /// build_id/symlink tree.
    fn write_project_root_manifest(project: &str, project_root: &std::path::Path) {
        let home = dirs::home_dir().unwrap();
        let dir = home.join(".coast").join("images").join(project);
        fs::create_dir_all(&dir).unwrap();
        let manifest = serde_json::json!({
            "project_root": project_root.to_string_lossy(),
        });
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_index_returns_html() {
        let app = test_app();

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers().get("content-type").unwrap();
        assert!(content_type.to_str().unwrap().contains("text/html"));
    }

    #[tokio::test]
    async fn test_ls_empty() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ls")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["instances"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_ls_with_project_filter() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ls?project=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["instances"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_stop_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stop")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"nonexistent","project":"test"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_invalid_json_returns_error() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stop")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(
            response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn test_cors_headers() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/v1/ls")
                    .header("origin", "http://localhost:3000")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response
            .headers()
            .contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn test_404_or_spa_fallback_for_unknown_route() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // When coast-guard/dist exists, the SPA fallback serves index.html (200).
        // Otherwise, axum returns 404.
        let status = response.status();
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::OK,
            "expected 404 or 200 (SPA fallback), got {status}"
        );
    }

    #[tokio::test]
    async fn test_event_bus_emit_and_receive() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();

        state.emit_event(CoastEvent::InstanceStopped {
            name: "dev-1".to_string(),
            project: "test-proj".to_string(),
        });

        let event = rx.recv().await.unwrap();
        match event {
            CoastEvent::InstanceStopped { name, project } => {
                assert_eq!(name, "dev-1");
                assert_eq!(project, "test-proj");
            }
            _ => panic!("unexpected event variant"),
        }
    }

    #[tokio::test]
    async fn test_event_bus_multiple_subscribers() {
        let state = test_state();
        let mut rx1 = state.event_bus.subscribe();
        let mut rx2 = state.event_bus.subscribe();

        state.emit_event(CoastEvent::InstanceCreated {
            name: "dev-2".to_string(),
            project: "myapp".to_string(),
            remote_host: None,
        });

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();

        let json1 = serde_json::to_string(&e1).unwrap();
        let json2 = serde_json::to_string(&e2).unwrap();
        assert_eq!(json1, json2);
        assert!(json1.contains("instance.created"));
        assert!(json1.contains("dev-2"));
    }

    #[tokio::test]
    async fn test_event_bus_no_subscribers_doesnt_panic() {
        let state = test_state();
        state.emit_event(CoastEvent::BuildStarted {
            project: "test".to_string(),
        });
    }

    #[tokio::test]
    async fn test_coast_event_serialization() {
        let event = CoastEvent::InstanceAssigned {
            name: "dev-1".to_string(),
            project: "filemap".to_string(),
            worktree: "feature-x".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"instance.assigned\""));
        assert!(json.contains("\"worktree\":\"feature-x\""));

        let roundtrip: CoastEvent = serde_json::from_str(&json).unwrap();
        match roundtrip {
            CoastEvent::InstanceAssigned {
                name,
                project,
                worktree,
            } => {
                assert_eq!(name, "dev-1");
                assert_eq!(project, "filemap");
                assert_eq!(worktree, "feature-x");
            }
            _ => panic!("unexpected variant after roundtrip"),
        }
    }

    #[tokio::test]
    async fn test_websocket_upgrade_request() {
        let state = test_state();
        let app = api::api_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let url = format!("ws://127.0.0.1:{}/api/v1/events", addr.port());
        let result = tokio_tungstenite::connect_async(&url).await;

        assert!(
            result.is_ok(),
            "WebSocket connection should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_websocket_receives_events() {
        let state = test_state();
        let state_clone = Arc::clone(&state);
        let app = api::api_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let url = format!("ws://127.0.0.1:{}/api/v1/events", addr.port());
        let (mut ws_stream, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        state_clone.emit_event(CoastEvent::InstanceStarted {
            name: "ws-test".to_string(),
            project: "test-proj".to_string(),
            remote_host: None,
        });

        use futures_util::StreamExt;
        let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), ws_stream.next())
            .await
            .expect("should receive message within 2s")
            .expect("stream should not end")
            .expect("message should not be error");

        let text = msg.into_text().unwrap();
        let event: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(event["event"], "instance.started");
        assert_eq!(event["name"], "ws-test");
        assert_eq!(event["project"], "test-proj");
    }

    #[tokio::test]
    async fn test_exec_agent_shell_available_false_when_not_configured() {
        let project = format!("agent-shell-avail-false-{}", uuid::Uuid::new_v4().simple());
        let name = "dev-1";
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                &project,
                name,
                Some("missing-build".to_string()),
            ))
            .unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/exec/agent-shell?project={}&name={}",
                        project, name
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["available"], false);

        remove_project_images_dir(&project);
    }

    #[tokio::test]
    async fn test_exec_agent_shell_available_true_when_manifest_has_command() {
        let project = format!("agent-shell-avail-true-{}", uuid::Uuid::new_v4().simple());
        let build_id = "build-test-1";
        let name = "dev-1";
        write_manifest_with_agent_command(&project, build_id, "echo hello");

        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(&project, name, Some(build_id.to_string())))
                .unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/exec/agent-shell?project={}&name={}",
                        project, name
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["available"], true);

        remove_project_images_dir(&project);
    }

    #[tokio::test]
    async fn test_exec_agent_shell_spawn_conflict_when_not_configured() {
        let project = format!(
            "agent-shell-spawn-conflict-{}",
            uuid::Uuid::new_v4().simple()
        );
        let name = "dev-1";
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                &project,
                name,
                Some("missing-build".to_string()),
            ))
            .unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/exec/agent-shell/spawn")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"project":"{}","name":"{}"}}"#,
                        project, name
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"]
            .as_str()
            .unwrap_or_default()
            .contains("No [agent_shell] command configured"));

        remove_project_images_dir(&project);
    }

    #[tokio::test]
    async fn test_exec_sessions_promotes_live_agent_when_active_is_stale() {
        let project = format!(
            "agent-shell-promote-stale-{}",
            uuid::Uuid::new_v4().simple()
        );
        let name = "dev-1";
        let stale_session_id = "stale-session";
        let live_session_id = "live-session";

        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(&project, name, None))
                .unwrap();
            let stale_id = db
                .create_agent_shell(&project, name, "claude --dangerously-skip-permissions")
                .unwrap();
            db.update_agent_shell_session_id(stale_id, stale_session_id)
                .unwrap();
            db.set_active_agent_shell(&project, name, stale_id).unwrap();

            let live_id = db
                .create_agent_shell(&project, name, "claude --dangerously-skip-permissions")
                .unwrap();
            db.update_agent_shell_session_id(live_id, live_session_id)
                .unwrap();
        }
        {
            let mut sessions = state.exec_sessions.lock().await;
            let (output_tx, _) = broadcast::channel::<Vec<u8>>(8);
            sessions.insert(
                live_session_id.to_string(),
                PtySession {
                    id: live_session_id.to_string(),
                    project: format!("{project}:{name}"),
                    child_pid: 0,
                    master_read_fd: -1,
                    master_write_fd: -1,
                    scrollback: Arc::new(Mutex::new(VecDeque::new())),
                    output_tx,
                },
            );
        }

        let state_for_assert = Arc::clone(&state);
        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/exec/sessions?project={}&name={}",
                        project, name
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let sessions: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let arr = sessions.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], live_session_id);
        assert_eq!(arr[0]["is_active_agent"], true);

        let db = state_for_assert.db.lock().await;
        let active = db.get_active_agent_shell(&project, name).unwrap().unwrap();
        assert_eq!(active.session_id.as_deref(), Some(live_session_id));
    }

    #[tokio::test]
    async fn test_exec_agent_shell_activate_sets_single_active() {
        let project = format!("agent-shell-activate-{}", uuid::Uuid::new_v4().simple());
        let name = "dev-1";
        let state = test_state();
        let shell2_row_id;
        let shell2_local_id;
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(&project, name, None))
                .unwrap();
            let shell1_row_id = db.create_agent_shell(&project, name, "claude").unwrap();
            shell2_row_id = db.create_agent_shell(&project, name, "claude").unwrap();
            shell2_local_id = db
                .get_agent_shell_by_id(shell2_row_id)
                .unwrap()
                .unwrap()
                .shell_id;
            db.set_active_agent_shell(&project, name, shell1_row_id)
                .unwrap();
        }

        let state_for_assert = Arc::clone(&state);
        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/exec/agent-shell/activate")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"project":"{}","name":"{}","shell_id":{}}}"#,
                        project, name, shell2_local_id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["shell_id"].as_i64(), Some(shell2_local_id));
        assert_eq!(json["is_active_agent"], true);

        let db = state_for_assert.db.lock().await;
        let active = db.get_active_agent_shell(&project, name).unwrap().unwrap();
        assert_eq!(active.id, shell2_row_id);
        assert_eq!(active.shell_id, shell2_local_id);
        let shells = db.list_agent_shells(&project, name).unwrap();
        assert_eq!(shells.iter().filter(|s| s.is_active).count(), 1);
    }

    #[tokio::test]
    async fn test_exec_agent_shell_close_removes_shell_and_session() {
        let project = format!("agent-shell-close-{}", uuid::Uuid::new_v4().simple());
        let name = "dev-1";
        let session_id = "agent-close-session";
        let state = test_state();
        let target_row_id;
        let target_local_id;
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(&project, name, None))
                .unwrap();
            let active_row_id = db.create_agent_shell(&project, name, "claude").unwrap();
            target_row_id = db.create_agent_shell(&project, name, "claude").unwrap();
            target_local_id = db
                .get_agent_shell_by_id(target_row_id)
                .unwrap()
                .unwrap()
                .shell_id;
            db.set_active_agent_shell(&project, name, active_row_id)
                .unwrap();
            db.update_agent_shell_session_id(target_row_id, session_id)
                .unwrap();
        }
        {
            let mut sessions = state.exec_sessions.lock().await;
            let (output_tx, _) = broadcast::channel::<Vec<u8>>(8);
            sessions.insert(
                session_id.to_string(),
                PtySession {
                    id: session_id.to_string(),
                    project: format!("{project}:{name}"),
                    child_pid: 999_999,
                    master_read_fd: -1,
                    master_write_fd: -1,
                    scrollback: Arc::new(Mutex::new(VecDeque::new())),
                    output_tx,
                },
            );
        }

        let state_for_assert = Arc::clone(&state);
        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/exec/agent-shell/close")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"project":"{}","name":"{}","shell_id":{}}}"#,
                        project, name, target_local_id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["shell_id"].as_i64(), Some(target_local_id));
        assert_eq!(json["closed"], true);

        let db = state_for_assert.db.lock().await;
        assert!(db.get_agent_shell_by_id(target_row_id).unwrap().is_none());
        assert_eq!(db.list_agent_shells(&project, name).unwrap().len(), 1);
        drop(db);

        let sessions = state_for_assert.exec_sessions.lock().await;
        assert!(!sessions.contains_key(session_id));
    }

    #[tokio::test]
    async fn test_exec_agent_shell_close_rejects_instance_mismatch() {
        let project = format!(
            "agent-shell-close-mismatch-{}",
            uuid::Uuid::new_v4().simple()
        );
        let state = test_state();
        let foreign_shell_row_id;
        let foreign_shell_local_id;
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(&project, "dev-1", None))
                .unwrap();
            db.insert_instance(&make_instance(&project, "dev-2", None))
                .unwrap();
            foreign_shell_row_id = db.create_agent_shell(&project, "dev-2", "claude").unwrap();
            foreign_shell_local_id = db
                .get_agent_shell_by_id(foreign_shell_row_id)
                .unwrap()
                .unwrap()
                .shell_id;
        }

        let state_for_assert = Arc::clone(&state);
        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/exec/agent-shell/close")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"project":"{}","name":"dev-1","shell_id":{}}}"#,
                        project, foreign_shell_local_id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not found"));

        let db = state_for_assert.db.lock().await;
        assert!(db
            .get_agent_shell_by_id(foreign_shell_row_id)
            .unwrap()
            .is_some());
    }

    // -----------------------------------------------------------------------
    // Settings CRUD tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_setting_missing_key() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/settings?key=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["key"], "nonexistent");
        assert!(json["value"].is_null());
    }

    #[tokio::test]
    async fn test_set_and_get_setting() {
        let state = test_state();
        let app1 = api::api_router(state.clone());

        let response = app1
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/settings")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"key":"theme","value":"dark"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["key"], "theme");
        assert_eq!(json["value"], "dark");

        let app2 = api::api_router(state);
        let response = app2
            .oneshot(
                Request::builder()
                    .uri("/api/v1/settings?key=theme")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["key"], "theme");
        assert_eq!(json["value"], "dark");
    }

    // -----------------------------------------------------------------------
    // Shared services tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_shared_ls_all_empty() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/shared/ls-all")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["projects"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_shared_ls_all_with_services() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("test", "postgres", Some("pg-1"), "running")
                .unwrap();
            db.insert_shared_service("test", "redis", None, "stopped")
                .unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/shared/ls-all")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let projects = json["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["project"], "test");
        assert_eq!(projects[0]["total"], 2);
        assert_eq!(projects[0]["running"], 1);
    }

    // -----------------------------------------------------------------------
    // Builds list tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_builds_ls_empty() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/builds?project=nonexistent-project-xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let builds = json["builds"].as_array().unwrap();
        assert!(builds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Error paths for container-dependent endpoints
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_images_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/images?project=x&name=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_images_stopped_instance() {
        let state = test_state();
        {
            let mut inst = make_instance("x", "stopped", None);
            inst.status = InstanceStatus::Stopped;
            state.db.lock().await.insert_instance(&inst).unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/images?project=x&name=stopped")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_files_tree_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/files/tree?project=x&name=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_volumes_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/volumes?project=x&name=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_secrets_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/secrets?project=x&name=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // base64_encode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(crate::api::query::files::base64_encode(""), "");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(crate::api::query::files::base64_encode("hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_encode_no_padding() {
        assert_eq!(crate::api::query::files::base64_encode("abc"), "YWJj");
    }

    #[test]
    fn test_base64_encode_one_byte_padding() {
        assert_eq!(crate::api::query::files::base64_encode("ab"), "YWI=");
    }

    #[test]
    fn test_base64_encode_special_chars() {
        let input = "hello\nworld\t!";
        let encoded = crate::api::query::files::base64_encode(input);
        assert_eq!(encoded, "aGVsbG8Kd29ybGQJIQ==");
    }

    // -----------------------------------------------------------------------
    // resolve_coast_container tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_resolve_coast_container_not_found() {
        let state = test_state();
        let result =
            crate::api::query::resolve_coast_container(&state, "proj", "nonexistent").await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resolve_coast_container_stopped() {
        let state = test_state();
        let mut inst = make_instance("proj", "stopped-inst", None);
        inst.status = InstanceStatus::Stopped;
        state.db.lock().await.insert_instance(&inst).unwrap();
        let result =
            crate::api::query::resolve_coast_container(&state, "proj", "stopped-inst").await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_resolve_coast_container_provisioning() {
        let state = test_state();
        let mut inst = make_instance("proj", "prov-inst", None);
        inst.status = InstanceStatus::Provisioning;
        state.db.lock().await.insert_instance(&inst).unwrap();
        let result = crate::api::query::resolve_coast_container(&state, "proj", "prov-inst").await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_resolve_coast_container_no_container_id() {
        let state = test_state();
        let mut inst = make_instance("proj", "no-id", None);
        inst.container_id = None;
        state.db.lock().await.insert_instance(&inst).unwrap();
        let result = crate::api::query::resolve_coast_container(&state, "proj", "no-id").await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_resolve_coast_container_running_success() {
        let state = test_state();
        let inst = make_instance("proj", "running-inst", Some("build-1".to_string()));
        state.db.lock().await.insert_instance(&inst).unwrap();
        let result =
            crate::api::query::resolve_coast_container(&state, "proj", "running-inst").await;
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved.container_id, "test-container");
        assert_eq!(resolved.build_id, Some("build-1".to_string()));
    }

    // -----------------------------------------------------------------------
    // to_api_response tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_to_api_response_not_found() {
        use axum::response::IntoResponse;
        use coast_core::protocol::{ErrorResponse, Response};

        let resp = Response::Error(ErrorResponse {
            error: "Instance 'x' not found".to_string(),
        });
        let http_resp = crate::api::routes::to_api_response(resp).into_response();
        assert_eq!(http_resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_to_api_response_conflict() {
        use axum::response::IntoResponse;
        use coast_core::protocol::{ErrorResponse, Response};

        let resp = Response::Error(ErrorResponse {
            error: "Instance already exists".to_string(),
        });
        let http_resp = crate::api::routes::to_api_response(resp).into_response();
        assert_eq!(http_resp.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_to_api_response_internal_error() {
        use axum::response::IntoResponse;
        use coast_core::protocol::{ErrorResponse, Response};

        let resp = Response::Error(ErrorResponse {
            error: "something went wrong".to_string(),
        });
        let http_resp = crate::api::routes::to_api_response(resp).into_response();
        assert_eq!(http_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_to_api_response_ok() {
        use axum::response::IntoResponse;
        use coast_core::protocol::{CheckoutResponse, Response};

        let resp = Response::Checkout(CheckoutResponse {
            checked_out: None,
            ports: vec![],
        });
        let http_resp = crate::api::routes::to_api_response(resp).into_response();
        assert_eq!(http_resp.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // POST endpoint error path tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_start_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/start")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"x","project":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_rm_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/rm")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"x","project":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_checkout_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/checkout")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"x","project":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Checkout with name "x" that doesn't exist returns error via to_api_response
        assert_ne!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_ps_nonexistent_instance() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ps")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"x","project":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().is_some());
    }

    // -----------------------------------------------------------------------
    // SSE streaming endpoint error tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_stream_build_invalid_path() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/build")
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .body(Body::from(
                        serde_json::json!({
                            "coastfile_path": "/nonexistent/Coastfile",
                            "refresh": false
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("event: error"),
            "Expected SSE error event in body: {body_str}"
        );
    }

    #[tokio::test]
    async fn test_stream_run_duplicate_instance() {
        let state = test_state_with_docker();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("dup-proj", "dup-inst", None))
                .unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/run")
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "dup-inst",
                            "project": "dup-proj"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("event: error"),
            "Expected SSE error event in body: {body_str}"
        );
        assert!(
            body_str.contains("already exists"),
            "Expected duplicate-instance error in body: {body_str}"
        );
        assert!(
            !body_str.contains("Host Docker is not available"),
            "Expected duplicate-instance error, not missing-Docker error: {body_str}"
        );
    }

    #[tokio::test]
    async fn test_rm_emits_stopping_status_changed() {
        let state = test_state();
        let inst = make_instance("proj", "rm-stop-test", None);
        state.db.lock().await.insert_instance(&inst).unwrap();

        let mut rx = state.event_bus.subscribe();

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/rm")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "rm-stop-test",
                            "project": "proj"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut saw_stopping = false;
        let mut saw_removed = false;
        while let Ok(event) = rx.try_recv() {
            let json = serde_json::to_value(&event).unwrap();
            let evt = json["event"].as_str().unwrap_or("");
            if evt == "instance.status_changed" {
                if json["status"].as_str() == Some("stopping") {
                    assert!(!saw_removed, "stopping event must arrive before removed");
                    saw_stopping = true;
                }
            }
            if evt == "instance.removed" {
                saw_removed = true;
            }
        }
        assert!(
            saw_stopping,
            "expected instance.status_changed with stopping"
        );
        assert!(saw_removed, "expected instance.removed event");
    }

    #[tokio::test]
    async fn test_docker_info_disconnected_without_docker() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/docker/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["connected"], false);
    }

    #[tokio::test]
    async fn test_open_docker_settings_route_exists() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/docker/open-settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // The route is registered as POST, so GET returns 405 (not 404).
        // This proves the route exists without triggering the handler
        // (which runs `open -a "Docker Desktop"` on macOS).
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_update_is_safe_to_update_reports_provisioning_blocker() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&CoastInstance {
                name: "prov-inst".to_string(),
                status: InstanceStatus::Provisioning,
                project: "proj".to_string(),
                branch: Some("main".to_string()),
                commit_sha: None,
                container_id: Some("test-container".to_string()),
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_host: None,
            })
            .unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/update/is-safe-to-update")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["safe"], false);
        assert_eq!(json["blockers"][0]["kind"], "instance_status");
    }

    #[tokio::test]
    async fn test_prepare_for_update_endpoint_returns_ready() {
        let state = test_state();
        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/update/prepare-for-update")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "timeout_ms": 100,
                            "close_sessions": false,
                            "stop_running_instances": false,
                            "stop_shared_services": false
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ready"], true);
        assert_eq!(json["report"]["safe"], true);
    }

    #[tokio::test]
    async fn test_start_rejected_while_update_quiescing() {
        let state = test_state();
        state.set_update_quiescing(true);
        {
            let db = state.db.lock().await;
            let mut inst = make_instance("quiesced-start", "proj", None);
            inst.status = InstanceStatus::Stopped;
            db.insert_instance(&inst).unwrap();
        }

        let app = api::api_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/start")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "quiesced-start",
                            "project": "proj"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"]
            .as_str()
            .unwrap_or_default()
            .contains("preparing for an update"));
    }

    #[tokio::test]
    async fn test_analytics_track_returns_no_content() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/track")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "event": "instance/stop",
                            "url": "http://localhost:5173/#/project/myapp"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_analytics_track_without_url() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/track")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "event": "button/click"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_analytics_track_missing_event_returns_error() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/track")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // -----------------------------------------------------------------------
    // GET /api/v1/docs/search integration tests
    // -----------------------------------------------------------------------

    async fn body_json(response: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_docs_search_returns_results() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search?q=coast")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert!(json.get("query").is_some());
        assert!(json.get("locale").is_some());
        assert!(json.get("strategy").is_some());
        assert!(json.get("results").unwrap().is_array());
    }

    #[tokio::test]
    async fn test_docs_search_nonexistent_returns_empty() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search?q=xyznonexistentterm123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let results = json.get("results").unwrap().as_array().unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_docs_search_missing_query_returns_400() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_docs_search_with_limit() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search?q=coast&limit=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let results = json.get("results").unwrap().as_array().unwrap();
        assert!(results.len() <= 1);
    }

    #[tokio::test]
    async fn test_docs_search_limit_clamped() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search?q=coast&limit=999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let results = json.get("results").unwrap().as_array().unwrap();
        assert!(results.len() <= 50);
    }

    #[tokio::test]
    async fn test_docs_search_with_language() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search?q=coast&language=es")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert!(json.get("locale").is_some());
    }

    #[tokio::test]
    async fn test_docs_search_result_shape() {
        let app = test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/docs/search?q=coast")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let results = json.get("results").unwrap().as_array().unwrap();
        if let Some(first) = results.first() {
            assert!(first.get("path").is_some(), "result should have 'path'");
            assert!(first.get("route").is_some(), "result should have 'route'");
            assert!(
                first.get("heading").is_some(),
                "result should have 'heading'"
            );
            assert!(
                first.get("snippet").is_some(),
                "result should have 'snippet'"
            );
            assert!(first.get("score").is_some(), "result should have 'score'");
        }
    }

    // -------------------------------------------------------------------
    // SSG endpoints (`GET /api/v1/ssg/builds`)
    // -------------------------------------------------------------------

    /// Fixture for SSG-endpoint router-level tests. Acquires the
    /// crate-wide `coast_home_env_lock`, points `COAST_HOME` at a
    /// fresh tempdir, and exposes helpers to seed
    /// `~/.coast/ssg/<project>/builds/<build_id>/manifest.json`.
    struct SsgEndpointFixture {
        _coast_home_guard: std::sync::MutexGuard<'static, ()>,
        prev_coast_home: Option<std::ffi::OsString>,
        _home: tempfile::TempDir,
        coast_home: std::path::PathBuf,
        state: Arc<crate::server::AppState>,
        project: String,
    }

    impl SsgEndpointFixture {
        fn new(project: &str) -> Self {
            let guard = crate::test_support::coast_home_env_lock();
            let prev_coast_home = std::env::var_os("COAST_HOME");
            let home = tempfile::tempdir().unwrap();
            let coast_home = home.path().join(".coast");
            fs::create_dir_all(&coast_home).unwrap();
            // Safety: serialized by `coast_home_env_lock`.
            unsafe {
                std::env::set_var("COAST_HOME", &coast_home);
            }

            let db = StateDb::open_in_memory().unwrap();
            let state = Arc::new(crate::server::AppState::new_for_testing(db));

            Self {
                _coast_home_guard: guard,
                prev_coast_home,
                _home: home,
                coast_home,
                state,
                project: project.to_string(),
            }
        }

        fn router(&self) -> axum::Router {
            api::api_router(self.state.clone())
        }

        fn builds_dir(&self) -> std::path::PathBuf {
            // Global pool — SSG artifacts are not per-project on disk.
            self.coast_home.join("ssg").join("builds")
        }

        /// Write a fully-formed manifest with the given
        /// `coastfile_hash`. The build_id is derived as
        /// `{coastfile_hash}_{built_at_compact}`. Returns the
        /// build_id so callers can wire `latest_build_id` / pins.
        ///
        /// The manifest matches the schema `SsgManifest` expects
        /// (every service includes `image`, `ports`, `env_keys`,
        /// `volumes`, `auto_create_db`) so consumers like
        /// `ps_ssg` that strict-parse the file can read it.
        fn write_manifest_full(
            &self,
            coastfile_hash: &str,
            built_at_rfc3339: &str,
            built_at_compact: &str,
            services: &[&str],
        ) -> String {
            let build_id = format!("{coastfile_hash}_{built_at_compact}");
            let dir = self.builds_dir().join(&build_id);
            fs::create_dir_all(&dir).unwrap();
            let services_json: Vec<serde_json::Value> = services
                .iter()
                .map(|name| {
                    serde_json::json!({
                        "name": name,
                        "image": format!("{name}:latest"),
                        "ports": [],
                        "env_keys": [],
                        "volumes": [],
                        "auto_create_db": false,
                    })
                })
                .collect();
            let manifest = serde_json::json!({
                "build_id": build_id,
                "coastfile_hash": coastfile_hash,
                "built_at": built_at_rfc3339,
                "services": services_json,
            });
            fs::write(
                dir.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            build_id
        }
    }

    impl Drop for SsgEndpointFixture {
        fn drop(&mut self) {
            // Safety: serialized by `_coast_home_guard`.
            match self.prev_coast_home.take() {
                Some(value) => unsafe { std::env::set_var("COAST_HOME", value) },
                None => unsafe { std::env::remove_var("COAST_HOME") },
            }
        }
    }

    async fn parse_json_body(response: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn test_ssg_builds_ls_empty_project_returns_ok_empty_list() {
        let fixture = SsgEndpointFixture::new("empty-cg");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/builds?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["project"], fixture.project);
        assert!(json["builds"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_ssg_builds_ls_returns_entries_sorted_desc() {
        let fixture = SsgEndpointFixture::new("sorted-cg");
        // Three builds, all sharing `coastfile_hash = "abc"`.
        let _old = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let _mid = fixture.write_manifest_full(
            "abc",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["postgres", "redis"],
        );
        let new = fixture.write_manifest_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres"],
        );

        // Anchor the project's hash via `latest_build_id`.
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &new).unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/builds?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        let builds = json["builds"].as_array().unwrap();
        assert_eq!(builds.len(), 3);
        assert_eq!(builds[0]["build_id"], "abc_20260422000000");
        assert_eq!(builds[1]["build_id"], "abc_20260421000000");
        assert_eq!(builds[2]["build_id"], "abc_20260420000000");
        assert_eq!(builds[1]["services_count"], 2);
        assert_eq!(builds[1]["services"][0], "postgres");
    }

    #[tokio::test]
    async fn test_ssg_builds_ls_marks_latest_and_pinned() {
        let fixture = SsgEndpointFixture::new("flags-cg");
        let a = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let b = fixture.write_manifest_full(
            "abc",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["postgres"],
        );
        let c = fixture.write_manifest_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres"],
        );

        // Seed `latest_build_id = c` and pin `b`.
        {
            let db = fixture.state.db.lock().await;
            use coast_ssg::state::SsgStateExt;
            db.set_latest_build_id(&fixture.project, &c).unwrap();
            db.upsert_ssg_consumer_pin(&coast_ssg::state::SsgConsumerPinRecord {
                project: fixture.project.clone(),
                build_id: b.clone(),
                created_at: "2026-04-21T00:01:00+00:00".to_string(),
            })
            .unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/builds?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        let builds = json["builds"].as_array().unwrap();
        let by_id: std::collections::HashMap<&str, &serde_json::Value> = builds
            .iter()
            .map(|e| (e["build_id"].as_str().unwrap(), e))
            .collect();
        assert_eq!(by_id[c.as_str()]["latest"], true);
        assert_eq!(by_id[c.as_str()]["pinned"], false);
        assert_eq!(by_id[b.as_str()]["pinned"], true);
        assert_eq!(by_id[b.as_str()]["latest"], false);
        assert_eq!(by_id[a.as_str()]["latest"], false);
        assert_eq!(by_id[a.as_str()]["pinned"], false);
    }

    #[tokio::test]
    async fn test_ssg_builds_ls_filters_to_requested_project() {
        // Two distinct SSG Coastfile families share one global
        // `~/.coast/ssg/builds/` pool. The endpoint must only
        // return builds belonging to `scope-cg`'s coastfile_hash.
        let fixture = SsgEndpointFixture::new("scope-cg");
        let cg_a = fixture.write_manifest_full(
            "aaa",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres"],
        );
        let cg_b = fixture.write_manifest_full(
            "aaa",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["redis"],
        );
        let _other_a = fixture.write_manifest_full(
            "bbb",
            "2026-04-23T00:00:00+00:00",
            "20260423000000",
            &["mongo"],
        );

        // Anchor `scope-cg` on hash `aaa`.
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &cg_a).unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/builds?project=scope-cg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        let builds = json["builds"].as_array().unwrap();
        let ids: Vec<&str> = builds
            .iter()
            .map(|e| e["build_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec![cg_a.as_str(), cg_b.as_str()]);
        for entry in builds {
            assert_eq!(entry["project"], "scope-cg");
        }
    }

    // -------------------------------------------------------------------
    // SSG inspect endpoint (`GET /api/v1/ssg/builds/inspect`)
    // -------------------------------------------------------------------

    impl SsgEndpointFixture {
        /// Write the artifact triplet (`manifest.json` +
        /// `ssg-coastfile.toml` + `compose.yml`) for one build_id.
        /// Returns the build_id.
        fn write_artifact_full(
            &self,
            coastfile_hash: &str,
            built_at_rfc3339: &str,
            built_at_compact: &str,
            services_json: serde_json::Value,
            coastfile_toml: &str,
            compose_yml: &str,
        ) -> String {
            let build_id = format!("{coastfile_hash}_{built_at_compact}");
            let dir = self.builds_dir().join(&build_id);
            fs::create_dir_all(&dir).unwrap();
            let manifest = serde_json::json!({
                "build_id": build_id,
                "coastfile_hash": coastfile_hash,
                "built_at": built_at_rfc3339,
                "services": services_json,
            });
            fs::write(
                dir.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            fs::write(dir.join("ssg-coastfile.toml"), coastfile_toml).unwrap();
            fs::write(dir.join("compose.yml"), compose_yml).unwrap();
            build_id
        }
    }

    #[tokio::test]
    async fn test_ssg_builds_inspect_returns_full_payload() {
        let fixture = SsgEndpointFixture::new("inspect-cg");
        let services_json = serde_json::json!([
            {
                "name": "postgres",
                "image": "postgres:15",
                "ports": [5432],
                "env_keys": ["POSTGRES_USER", "POSTGRES_PASSWORD"],
                "volumes": ["pg_data:/var/lib/postgresql/data"],
                "auto_create_db": true,
            },
            {
                "name": "redis",
                "image": "redis:7",
                "ports": [6379],
                "env_keys": [],
                "volumes": [],
                "auto_create_db": false,
            },
        ]);
        let build_id = fixture.write_artifact_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            services_json,
            "[ssg]\nruntime = \"dind\"\n",
            "services:\n  postgres:\n    image: postgres:15\n",
        );

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/ssg/builds/inspect?project={}&build_id={}",
                        fixture.project, build_id,
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["project"], fixture.project);
        assert_eq!(json["build_id"], build_id);
        assert_eq!(json["coastfile_hash"], "abc");
        assert_eq!(json["built_at"], "2026-04-22T00:00:00+00:00");
        assert!(json["built_at_unix"].as_i64().unwrap() > 0);
        assert!(json["artifact_path"].as_str().unwrap().ends_with(&build_id));

        let services = json["services"].as_array().unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0]["name"], "postgres");
        assert_eq!(services[0]["image"], "postgres:15");
        assert_eq!(services[0]["ports"][0], 5432);
        assert_eq!(services[0]["env_keys"][0], "POSTGRES_USER");
        assert_eq!(
            services[0]["volumes"][0],
            "pg_data:/var/lib/postgresql/data"
        );
        assert_eq!(services[0]["auto_create_db"], true);
        assert_eq!(services[1]["auto_create_db"], false);

        assert!(json["coastfile"]
            .as_str()
            .unwrap()
            .contains("runtime = \"dind\""));
        assert!(json["compose"].as_str().unwrap().contains("postgres:15"));

        // No `latest_build_id` / pin seeded -> both flags false.
        assert_eq!(json["latest"], false);
        assert_eq!(json["pinned"], false);
    }

    #[tokio::test]
    async fn test_ssg_builds_inspect_marks_latest_and_pinned() {
        let fixture = SsgEndpointFixture::new("inspect-flags");
        let build_id = fixture.write_artifact_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            serde_json::json!([]),
            "",
            "",
        );

        // Seed both flags pointing at the same build.
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &build_id).unwrap();
            db.upsert_ssg_consumer_pin(&coast_ssg::state::SsgConsumerPinRecord {
                project: fixture.project.clone(),
                build_id: build_id.clone(),
                created_at: "2026-04-22T00:01:00+00:00".to_string(),
            })
            .unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/ssg/builds/inspect?project={}&build_id={}",
                        fixture.project, build_id,
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["latest"], true);
        assert_eq!(json["pinned"], true);
    }

    #[tokio::test]
    async fn test_ssg_builds_inspect_returns_404_for_missing_build() {
        let fixture = SsgEndpointFixture::new("inspect-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/ssg/builds/inspect?project={}&build_id=does_not_exist",
                        fixture.project,
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_ssg_builds_inspect_handles_missing_optional_files() {
        // Manifest only — no `ssg-coastfile.toml`, no `compose.yml`.
        let fixture = SsgEndpointFixture::new("inspect-partial");
        let build_id = "abc_20260422000000".to_string();
        let dir = fixture.builds_dir().join(&build_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "build_id": build_id,
                "coastfile_hash": "abc",
                "built_at": "2026-04-22T00:00:00+00:00",
                "services": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/ssg/builds/inspect?project={}&build_id={}",
                        fixture.project, build_id,
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert!(json["coastfile"].is_null());
        assert!(json["compose"].is_null());
    }

    #[tokio::test]
    async fn test_ssg_builds_inspect_missing_param_returns_400() {
        let fixture = SsgEndpointFixture::new("inspect-bad-args");

        // Missing build_id.
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/builds/inspect?project=x")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Missing project.
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/builds/inspect?build_id=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_builds_ls_missing_project_param_returns_400() {
        let fixture = SsgEndpointFixture::new("missing-param");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/builds")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // axum's `Query` extractor returns 400 when a required field is
        // missing from the querystring; that's what we want here.
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // -------------------------------------------------------------------
    // SSG SSE endpoint (`POST /api/v1/stream/ssg-build`)
    // -------------------------------------------------------------------

    /// Parse an SSE response body into `(event_name, data)` frames.
    /// The daemon emits each frame as `event: <name>\ndata: <json>\n\n`.
    /// Comment / heartbeat lines (`:keep-alive`) are skipped. Returns
    /// `(name, data)` pairs in stream order.
    fn parse_sse_frames(bytes: &[u8]) -> Vec<(String, String)> {
        let body = String::from_utf8_lossy(bytes);
        let mut frames: Vec<(String, String)> = Vec::new();
        for raw_frame in body.split("\n\n") {
            let raw_frame = raw_frame.trim();
            if raw_frame.is_empty() {
                continue;
            }
            let mut event = String::new();
            let mut data = String::new();
            for line in raw_frame.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(rest) = line.strip_prefix("event:") {
                    event = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(rest.trim_start());
                }
                // Ignore `:keep-alive` and any other field types.
            }
            if !event.is_empty() {
                frames.push((event, data));
            }
        }
        frames
    }

    #[test]
    fn test_parse_sse_frames_handles_progress_and_complete() {
        let body = b"event: progress\ndata: {\"step\":\"a\"}\n\nevent: complete\ndata: {\"build_id\":\"x\"}\n\n";
        let frames = parse_sse_frames(body);
        assert_eq!(
            frames,
            vec![
                ("progress".to_string(), "{\"step\":\"a\"}".to_string()),
                ("complete".to_string(), "{\"build_id\":\"x\"}".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn test_ssg_build_stream_rejects_get() {
        // `/api/v1/stream/ssg-build` is POST-only, mirroring `/build`
        // and `/remote-build`. Axum returns 405 for the wrong verb.
        let fixture = SsgEndpointFixture::new("ssg-build-get");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/stream/ssg-build")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_ssg_run_stream_get_returns_405() {
        let fixture = SsgEndpointFixture::new("ssg-run-get");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/stream/ssg-run")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_ssg_run_stream_rejects_missing_project() {
        let fixture = SsgEndpointFixture::new("ssg-run-missing-project");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/ssg-run")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status().is_client_error(),
            "expected 4xx for missing project; got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn test_ssg_run_stream_emits_error_frame_when_no_build() {
        // No SSG runtime row + no build → the run pipeline emits a
        // single `event: error` SSE frame with a clear message and
        // closes the stream. SSE endpoints always return 200 OK
        // (the stream itself opens successfully); the error is
        // surfaced inside the stream body.
        let fixture = SsgEndpointFixture::new("ssg-run-no-build");
        let body = serde_json::json!({ "project": fixture.project });

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/ssg-run")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let frames = parse_sse_frames(&bytes);
        let error_frame = frames
            .iter()
            .find(|(event, _)| event == "error")
            .expect("expected an `error` SSE frame");
        assert!(
            error_frame.1.contains("no SSG build"),
            "expected error to mention the missing build; got {}",
            error_frame.1,
        );
    }

    #[tokio::test]
    async fn test_ssg_build_stream_rejects_missing_project() {
        // axum's `Json` extractor returns 4xx when the required
        // `project` field is missing from the body. The exact status
        // is implementation-defined (typically 422 UNPROCESSABLE_ENTITY
        // for serde failures); both 400 and 422 are acceptable as
        // "client error".
        let fixture = SsgEndpointFixture::new("ssg-build-missing-project");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/ssg-build")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status().is_client_error(),
            "expected 4xx for missing project; got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn test_ssg_build_stream_rejects_when_no_ssg_coastfile() {
        // Point `working_dir` at a tempdir that has no SSG Coastfile.
        // SSE returns 200 (the stream itself starts), then emits a
        // single `event: error` frame with a clear message before
        // closing.
        let fixture = SsgEndpointFixture::new("ssg-build-no-coastfile");
        let empty_dir = tempfile::tempdir().unwrap();

        let body = serde_json::json!({
            "project": fixture.project,
            "working_dir": empty_dir.path(),
        });

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/ssg-build")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let frames = parse_sse_frames(&bytes);

        // We expect exactly one error frame and no `complete` frame.
        // Any number of `progress` frames are tolerated as long as
        // the stream terminates with an `error`.
        assert!(
            frames.iter().any(|(ev, _)| ev == "error"),
            "no `error` frame in SSE body: {frames:?}"
        );
        assert!(
            !frames.iter().any(|(ev, _)| ev == "complete"),
            "unexpected `complete` frame: {frames:?}"
        );
    }

    #[tokio::test]
    async fn test_ssg_build_stream_rejects_empty_project_string() {
        // The handler also enforces a non-empty `project` server-side
        // (in addition to whatever serde does). An empty string
        // bypasses serde but should still be rejected.
        let fixture = SsgEndpointFixture::new("ssg-build-empty-project");
        let body = serde_json::json!({ "project": "" });

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/ssg-build")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // The stream itself opens (200 OK); the failure is reported
        // inside the SSE body as an `error` event.
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let frames = parse_sse_frames(&bytes);
        assert!(
            frames
                .iter()
                .any(|(ev, data)| ev == "error" && data.contains("project")),
            "missing project-related error frame: {frames:?}"
        );
    }

    #[tokio::test]
    async fn test_ssg_build_stream_unknown_fields_are_ignored() {
        // Forward-compat: extra request fields not declared on
        // `SsgBuildSseRequest` must NOT cause deserialization to
        // fail. Serde's default behaviour permits unknown fields.
        let fixture = SsgEndpointFixture::new("ssg-build-unknown-fields");
        let empty_dir = tempfile::tempdir().unwrap();

        let body = serde_json::json!({
            "project": fixture.project,
            "working_dir": empty_dir.path(),
            "this_field_does_not_exist": 123,
            "another_extra": "string",
        });

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/stream/ssg-build")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Body deserialization succeeded (200), and we get an SSE
        // stream that ultimately errors on the missing Coastfile —
        // that's fine, what we're verifying is that the unknown
        // fields didn't cause a 4xx.
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -------------------------------------------------------------------
    // SSG lifecycle endpoints (run/start/stop/rm). Real-Docker happy
    // paths require integration tests; these focus on validation +
    // pre-Docker error paths.
    // -------------------------------------------------------------------

    fn lifecycle_body(project: &str) -> Body {
        Body::from(serde_json::json!({ "project": project }).to_string())
    }

    fn rm_body(project: &str, with_data: bool, force: bool) -> Body {
        Body::from(
            serde_json::json!({
                "project": project,
                "with_data": with_data,
                "force": force,
            })
            .to_string(),
        )
    }

    #[tokio::test]
    async fn test_ssg_run_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("run-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/run")
                    .header("content-type", "application/json")
                    .body(lifecycle_body(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_run_returns_409_when_no_build() {
        let fixture = SsgEndpointFixture::new("run-no-build");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/run")
                    .header("content-type", "application/json")
                    .body(lifecycle_body(&fixture.project))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_ssg_start_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("start-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/start")
                    .header("content-type", "application/json")
                    .body(lifecycle_body(""))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_start_returns_409_when_no_ssg_row() {
        let fixture = SsgEndpointFixture::new("start-no-ssg");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/start")
                    .header("content-type", "application/json")
                    .body(lifecycle_body(&fixture.project))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_ssg_stop_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("stop-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/stop")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "project": "", "force": false }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // -------------------------------------------------------------------
    // Per-service inner-compose endpoints (services/{stop,start,
    // restart,rm}). Validation paths only — actual `docker compose`
    // invocations need a running SSG and are exercised in the live
    // smoke tests against `cg-ssg`.
    // -------------------------------------------------------------------

    fn service_action_body(project: &str, service: &str) -> Body {
        Body::from(serde_json::json!({ "project": project, "service": service }).to_string())
    }

    #[tokio::test]
    async fn test_ssg_service_stop_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("svc-stop-missing-project");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/services/stop")
                    .header("content-type", "application/json")
                    .body(service_action_body("", "postgres"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_service_start_missing_service_returns_400() {
        let fixture = SsgEndpointFixture::new("svc-start-missing-service");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/services/start")
                    .header("content-type", "application/json")
                    .body(service_action_body(&fixture.project, ""))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_service_restart_returns_404_when_no_ssg_row() {
        // No SSG runtime row → resolve_ssg_container_id returns
        // 404 with a clear message.
        let fixture = SsgEndpointFixture::new("svc-restart-no-ssg");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/services/restart")
                    .header("content-type", "application/json")
                    .body(service_action_body(&fixture.project, "postgres"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_ssg_service_rm_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("svc-rm-missing-project");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/services/rm")
                    .header("content-type", "application/json")
                    .body(service_action_body("", "redis"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_rm_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("rm-lifecycle-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/rm")
                    .header("content-type", "application/json")
                    .body(rm_body("", false, false))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // -------------------------------------------------------------------
    // SSG Local Page endpoints: images, volumes, ws-terminal, ws-logs,
    // ws-stats. Real-Docker happy paths require `make verify`-style
    // integration; these tests focus on validation + 404/409 paths.
    // -------------------------------------------------------------------

    /// Seed the in-memory state.db with a fake `ssg` row so the
    /// `resolve_ssg_container_id` helper finds a valid container_id.
    /// Used for tests that need a "running SSG" precondition (the
    /// actual Docker exec then fails since the container doesn't
    /// exist — that's intentional, the test stops before that).
    async fn seed_ssg_row(state: &Arc<crate::server::AppState>, project: &str, container_id: &str) {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        db.upsert_ssg(project, "running", Some(container_id), Some("abc_x"))
            .unwrap();
    }

    #[tokio::test]
    async fn test_ssg_images_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("images-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/images")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_images_returns_404_when_no_ssg_row() {
        let fixture = SsgEndpointFixture::new("images-no-ssg");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/images?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_ssg_volumes_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("volumes-missing");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/volumes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ssg_volumes_returns_404_when_no_ssg_row() {
        let fixture = SsgEndpointFixture::new("volumes-no-ssg");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/volumes?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_ssg_images_returns_409_when_ssg_row_has_no_container() {
        let fixture = SsgEndpointFixture::new("images-no-cid");
        // Seed an `ssg` row but with `container_id = NULL` (the
        // `built` status with no run yet).
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.upsert_ssg(&fixture.project, "built", None, Some("abc_x"))
                .unwrap();
        }
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/images?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_ws_ssg_terminal_rejects_get_without_upgrade_header() {
        let fixture = SsgEndpointFixture::new("ws-term");
        seed_ssg_row(&fixture.state, &fixture.project, "fake-container").await;
        // Plain GET (no `Upgrade: websocket`) — axum's
        // `WebSocketUpgrade` extractor returns 400/426/upgrade-required.
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/terminal?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status().is_client_error()
                || response.status() == StatusCode::UPGRADE_REQUIRED,
            "expected 4xx for non-WS request; got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn test_ws_ssg_logs_rejects_get_without_upgrade_header() {
        let fixture = SsgEndpointFixture::new("ws-logs");
        seed_ssg_row(&fixture.state, &fixture.project, "fake-container").await;
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/ssg/logs/stream?project={}",
                        fixture.project
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status().is_client_error()
                || response.status() == StatusCode::UPGRADE_REQUIRED,
            "expected 4xx for non-WS request; got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn test_ws_ssg_stats_rejects_get_without_upgrade_header() {
        let fixture = SsgEndpointFixture::new("ws-stats");
        seed_ssg_row(&fixture.state, &fixture.project, "fake-container").await;
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/ssg/stats/stream?project={}",
                        fixture.project
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.status().is_client_error()
                || response.status() == StatusCode::UPGRADE_REQUIRED,
            "expected 4xx for non-WS request; got {}",
            response.status()
        );
    }

    // Note on missing-SSG behavior for WS endpoints: axum's
    // `WebSocketUpgrade` extractor runs BEFORE our `Query`
    // extractor, so a plain GET without an `Upgrade: websocket`
    // header returns 400 from the WS-extractor regardless of
    // whether the project has an SSG row. The 404 path is
    // exercised by the live SPA flow (real WS handshake →
    // resolver runs → 404 if no SSG). For unit tests that
    // particular path requires a real websocket client, which is
    // out of scope here (consistent with the rest of the daemon's
    // ws_* test conventions).

    // -------------------------------------------------------------------
    // SSG state endpoint (`GET /api/v1/ssg/state`)
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn test_ssg_state_returns_empty_for_unknown_project() {
        // No `ssg` row for the project -> `Ps` returns "No SSG for
        // project '...'" with empty services + null status. The
        // endpoint should still respond 200 with a structured
        // empty-ish payload (not 404), so the SPA can render an
        // "SSG not built yet" empty state without a dedicated
        // error path.
        let fixture = SsgEndpointFixture::new("state-empty");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/state?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["project"], fixture.project);
        assert!(json["services"].as_array().unwrap().is_empty());
        assert!(json["ports"].as_array().unwrap().is_empty());
        assert!(json["latest_build_id"].is_null());
        assert!(json["pinned_build_id"].is_null());
    }

    #[tokio::test]
    async fn test_ssg_state_includes_latest_and_pin_info() {
        let fixture = SsgEndpointFixture::new("state-flags");
        let build_id = fixture.write_manifest_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres"],
        );

        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &build_id).unwrap();
            db.upsert_ssg_consumer_pin(&coast_ssg::state::SsgConsumerPinRecord {
                project: fixture.project.clone(),
                build_id: build_id.clone(),
                created_at: "2026-04-22T00:01:00+00:00".to_string(),
            })
            .unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/ssg/state?project={}", fixture.project))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["latest_build_id"], build_id);
        assert_eq!(json["pinned_build_id"], build_id);
    }

    #[tokio::test]
    async fn test_ssg_state_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("state-bad-args");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ssg/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // -------------------------------------------------------------------
    // SSG remove endpoint (`POST /api/v1/ssg/builds/rm`)
    // -------------------------------------------------------------------

    fn rm_request_body(project: &str, build_ids: &[&str]) -> Body {
        let body = serde_json::json!({
            "project": project,
            "build_ids": build_ids,
        });
        Body::from(body.to_string())
    }

    #[tokio::test]
    async fn test_ssg_builds_rm_removes_artifact_dirs() {
        let fixture = SsgEndpointFixture::new("rm-cg");
        let a = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let b = fixture.write_manifest_full(
            "abc",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["postgres"],
        );
        // Confirm artifacts exist on disk.
        assert!(fixture.builds_dir().join(&a).exists());
        assert!(fixture.builds_dir().join(&b).exists());

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/builds/rm")
                    .header("content-type", "application/json")
                    .body(rm_request_body(&fixture.project, &[&a, &b]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        let removed: Vec<&str> = json["removed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&a.as_str()));
        assert!(removed.contains(&b.as_str()));
        assert!(json["skipped_pinned"].as_array().unwrap().is_empty());
        assert!(json["errors"].as_array().unwrap().is_empty());

        // Filesystem actually emptied.
        assert!(!fixture.builds_dir().join(&a).exists());
        assert!(!fixture.builds_dir().join(&b).exists());
    }

    #[tokio::test]
    async fn test_ssg_builds_rm_skips_pinned() {
        let fixture = SsgEndpointFixture::new("rm-pin");
        let a = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let b = fixture.write_manifest_full(
            "abc",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["postgres"],
        );

        // Pin `b` so the deletion request must skip it.
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.upsert_ssg_consumer_pin(&coast_ssg::state::SsgConsumerPinRecord {
                project: fixture.project.clone(),
                build_id: b.clone(),
                created_at: "2026-04-21T00:01:00+00:00".to_string(),
            })
            .unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/builds/rm")
                    .header("content-type", "application/json")
                    .body(rm_request_body(&fixture.project, &[&a, &b]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        let removed: Vec<&str> = json["removed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let skipped: Vec<&str> = json["skipped_pinned"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(removed, vec![a.as_str()]);
        assert_eq!(skipped, vec![b.as_str()]);
        // Pinned artifact still on disk.
        assert!(fixture.builds_dir().join(&b).exists());
        assert!(!fixture.builds_dir().join(&a).exists());
    }

    #[tokio::test]
    async fn test_ssg_builds_rm_clears_latest_when_removed() {
        let fixture = SsgEndpointFixture::new("rm-latest");
        let a = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        // Anchor `latest_build_id = a`.
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &a).unwrap();
        }

        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/builds/rm")
                    .header("content-type", "application/json")
                    .body(rm_request_body(&fixture.project, &[&a]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["cleared_latest"], true);

        // Verify `latest_build_id` is now NULL in state.db.
        {
            use coast_ssg::state::SsgStateExt;
            let db = fixture.state.db.lock().await;
            let row = db.get_ssg(&fixture.project).unwrap();
            assert!(row.is_some());
            assert!(row.unwrap().latest_build_id.is_none());
        }
    }

    #[tokio::test]
    async fn test_ssg_builds_rm_idempotent_for_missing_dir() {
        // Removing a build that doesn't exist on disk is treated as
        // success (idempotent) so refreshes after concurrent deletes
        // don't surface spurious errors.
        let fixture = SsgEndpointFixture::new("rm-idempotent");
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/builds/rm")
                    .header("content-type", "application/json")
                    .body(rm_request_body(&fixture.project, &["abc_doesnotexist"]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = parse_json_body(response).await;
        assert_eq!(json["removed"][0], "abc_doesnotexist");
        assert!(json["errors"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_ssg_builds_rm_missing_project_returns_400() {
        let fixture = SsgEndpointFixture::new("rm-bad-args");
        let body = serde_json::json!({
            "project": "",
            "build_ids": ["abc_x"],
        });
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ssg/builds/rm")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Regression test for the user-reported bug where the SPA build picker
    /// listed `shared_service_groups` alongside the regular Coastfile. The
    /// daemon must filter both `shared_service_groups` (SSG variant, built
    /// via `coast ssg build`) and `remote*` (remote variant, built via
    /// `coast remote build`) out of the response so the picker only sees
    /// types that `coast build` accepts.
    #[tokio::test]
    async fn test_builds_coastfile_types_omits_shared_service_groups_and_remote() {
        let project = format!("ctypes-filter-{}", uuid::Uuid::new_v4().simple());

        let project_root = tempfile::tempdir().unwrap();
        for fname in [
            "Coastfile",
            "Coastfile.light",
            "Coastfile.remote.toml",
            "Coastfile.shared_service_groups",
        ] {
            fs::write(project_root.path().join(fname), b"# fixture").unwrap();
        }
        write_project_root_manifest(&project, project_root.path());

        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/builds/coastfile-types?project={project}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let types: Vec<String> = json["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        assert_eq!(
            types,
            vec!["default".to_string(), "light".to_string()],
            "expected only buildable variants; got {types:?}"
        );

        remove_project_images_dir(&project);
    }
}
