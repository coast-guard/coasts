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
            remote_name: None,
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
                remote_name: None,
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
}
