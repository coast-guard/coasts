use tracing::{debug, info};

use coast_core::error::Result;
use coast_core::protocol::BuildProgressEvent;
use coast_core::types::{InstanceStatus, PortMapping};

use crate::server::AppState;

use super::{emit, port_mappings_from_pre_allocated_ports};

/// Phase 3: Store port allocations, set primary port, transition status.
pub(super) async fn finalize_instance(
    state: &AppState,
    project: &str,
    instance_name: &str,
    container_id: &str,
    resolved_build_id: Option<&str>,
    pre_allocated_ports: &[(String, u16, u16)],
    final_status: &InstanceStatus,
    total_steps: u32,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<Vec<PortMapping>> {
    emit(
        progress,
        BuildProgressEvent::started("Allocating ports", total_steps, total_steps),
    );

    let db = state.db.lock().await;
    debug!(
        project = %project,
        instance = %instance_name,
        pre_allocated_port_count = pre_allocated_ports.len(),
        "finalizing instance with pre-allocated ports"
    );

    let mut ports: Vec<PortMapping> = Vec::new();
    for mapping in port_mappings_from_pre_allocated_ports(pre_allocated_ports) {
        db.insert_port_allocation(project, instance_name, &mapping)?;
        emit(
            progress,
            BuildProgressEvent::item(
                "Allocating ports",
                format!(
                    "{} :{} \u{2192} :{}",
                    mapping.logical_name, mapping.canonical_port, mapping.dynamic_port
                ),
                "ok",
            ),
        );
        ports.push(mapping);
    }

    if let Some(bid) = resolved_build_id {
        let key = crate::handlers::ports::primary_port_settings_key(project, bid);
        if db.get_setting(&key)?.is_none() && ports.len() == 1 {
            db.set_setting(&key, &ports[0].logical_name)?;
        }
    }

    db.update_instance_status(project, instance_name, final_status)?;
    state.emit_event(coast_core::protocol::CoastEvent::InstanceStatusChanged {
        name: instance_name.to_string(),
        project: project.to_string(),
        status: final_status.as_db_str().to_string(),
    });
    emit(progress, BuildProgressEvent::done("Allocating ports", "ok"));

    info!(
        name = %instance_name,
        project = %project,
        container_id = %container_id,
        "instance created and running"
    );

    Ok(ports)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::state::StateDb;
    use coast_core::protocol::CoastEvent;
    use coast_core::types::{CoastInstance, RuntimeType};
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Record};
    use tracing::subscriber::Interest;
    use tracing::{Event, Id, Metadata, Subscriber};

    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    fn test_state_with_instance(build_id: Option<&str>) -> AppState {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Provisioning,
            branch: None,
            commit_sha: None,
            container_id: Some("container-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: build_id.map(std::string::ToString::to_string),
            coastfile_type: None,
            remote_name: None,
        })
        .unwrap();
        AppState::new_for_testing(db)
    }

    #[tokio::test]
    async fn test_finalize_persists_ports_updates_status_and_emits_event() {
        let state = test_state_with_instance(None);
        let progress = discard_progress();
        let mut event_rx = state.event_bus.subscribe();

        let ports = finalize_instance(
            &state,
            "proj",
            "dev-1",
            "container-123",
            None,
            &[
                ("web".to_string(), 3000, 52340),
                ("api".to_string(), 8080, 52341),
            ],
            &InstanceStatus::Running,
            5,
            &progress,
        )
        .await
        .unwrap();

        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].logical_name, "web");
        assert_eq!(ports[1].logical_name, "api");

        let db = state.db.lock().await;
        let instance = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::Running);

        let allocations = db.get_port_allocations("proj", "dev-1").unwrap();
        assert_eq!(allocations.len(), 2);
        assert_eq!(allocations[0].logical_name, "api");
        assert_eq!(allocations[1].logical_name, "web");
        drop(db);

        let event = event_rx.recv().await.unwrap();
        assert!(matches!(
            event,
            CoastEvent::InstanceStatusChanged { name, project, status }
                if name == "dev-1" && project == "proj" && status == "running"
        ));
    }

    #[tokio::test]
    async fn test_finalize_sets_primary_port_for_single_port_when_missing() {
        let state = test_state_with_instance(Some("build-1"));
        let progress = discard_progress();

        finalize_instance(
            &state,
            "proj",
            "dev-1",
            "container-123",
            Some("build-1"),
            &[("web".to_string(), 3000, 52340)],
            &InstanceStatus::Running,
            5,
            &progress,
        )
        .await
        .unwrap();

        let db = state.db.lock().await;
        let key = crate::handlers::ports::primary_port_settings_key("proj", "build-1");
        assert_eq!(db.get_setting(&key).unwrap(), Some("web".to_string()));
    }

    #[tokio::test]
    async fn test_finalize_preserves_existing_primary_port_setting() {
        let state = test_state_with_instance(Some("build-1"));
        let key = crate::handlers::ports::primary_port_settings_key("proj", "build-1");
        {
            let db = state.db.lock().await;
            db.set_setting(&key, "api").unwrap();
        }

        let progress = discard_progress();
        finalize_instance(
            &state,
            "proj",
            "dev-1",
            "container-123",
            Some("build-1"),
            &[("web".to_string(), 3000, 52340)],
            &InstanceStatus::Running,
            5,
            &progress,
        )
        .await
        .unwrap();

        let db = state.db.lock().await;
        assert_eq!(db.get_setting(&key).unwrap(), Some("api".to_string()));
    }

    #[tokio::test]
    async fn test_finalize_does_not_set_primary_port_for_multiple_ports() {
        let state = test_state_with_instance(Some("build-1"));
        let progress = discard_progress();

        finalize_instance(
            &state,
            "proj",
            "dev-1",
            "container-123",
            Some("build-1"),
            &[
                ("web".to_string(), 3000, 52340),
                ("api".to_string(), 8080, 52341),
            ],
            &InstanceStatus::Running,
            5,
            &progress,
        )
        .await
        .unwrap();

        let db = state.db.lock().await;
        let key = crate::handlers::ports::primary_port_settings_key("proj", "build-1");
        assert_eq!(db.get_setting(&key).unwrap(), None);
    }

    #[derive(Clone, Default)]
    struct EventCapture(Arc<Mutex<Vec<HashMap<String, String>>>>);

    #[derive(Default)]
    struct FieldCaptureVisitor {
        fields: HashMap<String, String>,
    }

    impl Visit for FieldCaptureVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    #[derive(Clone)]
    struct CaptureSubscriber {
        events: EventCapture,
    }

    impl CaptureSubscriber {
        fn new(events: EventCapture) -> Self {
            Self { events }
        }
    }

    impl Subscriber for CaptureSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut visitor = FieldCaptureVisitor::default();
            event.record(&mut visitor);
            self.events.0.lock().unwrap().push(visitor.fields);
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}

        fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
            Interest::always()
        }

        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::INFO)
        }

        fn clone_span(&self, id: &Id) -> Id {
            id.clone()
        }

        fn try_close(&self, _id: Id) -> bool {
            true
        }
    }

    fn capture_finalize_events() -> Vec<HashMap<String, String>> {
        let events = EventCapture::default();
        let subscriber = CaptureSubscriber::new(events.clone());

        tracing::subscriber::with_default(subscriber, || {
            tracing::callsite::rebuild_interest_cache();
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let state = test_state_with_instance(None);
                    let progress = discard_progress();

                    finalize_instance(
                        &state,
                        "proj",
                        "dev-1",
                        "container-123",
                        None,
                        &[],
                        &InstanceStatus::Running,
                        5,
                        &progress,
                    )
                    .await
                    .unwrap();
                });
        });

        let captured = events.0.lock().unwrap().clone();
        captured
    }

    #[test]
    fn test_finalize_log_includes_container_id() {
        let mut captured = Vec::new();
        for _ in 0..3 {
            captured = capture_finalize_events();
            if let Some(final_log) = captured.iter().find(|fields| {
                fields
                    .get("message")
                    .map(|message| message.contains("instance created and running"))
                    .unwrap_or(false)
            }) {
                assert!(
                    final_log
                        .get("container_id")
                        .map(|value| value.contains("container-123"))
                        .unwrap_or(false),
                    "expected container_id field in final log event: {final_log:?}"
                );
                return;
            }
        }

        panic!("expected final run log event; captured events: {captured:?}");
    }
}
