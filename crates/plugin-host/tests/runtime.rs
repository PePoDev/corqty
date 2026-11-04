use chrono::{TimeDelta, Utc};
use corqty_plugin_host::{PluginManifest, RuntimePlugin};
use corqty_plugin_sdk::{ExecuteRequest, ExecutionContext};
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn manifest() -> PluginManifest {
    serde_json::from_str(include_str!("../../../plugins/example-wasm/manifest.json")).unwrap()
}
fn source() -> String {
    include_str!("../../../plugins/example-wasm/hello.wat").to_owned()
}
fn request() -> ExecuteRequest {
    ExecuteRequest {
        execution_id: Uuid::new_v4(),
        attempt: 1,
        workflow_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        node_id: "sample".into(),
        inputs: Default::default(),
        config: json!({}),
        deadline: Utc::now() + TimeDelta::seconds(10),
    }
}
fn context() -> ExecutionContext {
    ExecutionContext {
        cancel: CancellationToken::new(),
        scratch_dir: std::env::temp_dir(),
        logs: mpsc::channel(8).0,
        bound_secrets: HashMap::new(),
    }
}

#[tokio::test]
async fn precompiled_component_matches_source_and_runs() {
    let bytes = wat::parse_str(source()).unwrap();
    assert_eq!(
        bytes,
        include_bytes!("../../../plugins/example-wasm/hello.wasm")
    );
    let plugin = RuntimePlugin::new(manifest(), bytes, vec![]).await.unwrap();
    plugin.verify().await.unwrap();
    assert!(plugin.validate(&json!({})).await.unwrap().is_empty());
    let output = plugin.execute_runtime(request(), &context()).await.unwrap();
    assert_eq!(output.get("result"), Some(&json!("hello from wasm")));
}
#[tokio::test]
async fn rejects_modules_native_code_and_changed_metadata() {
    for bytes in [
        b"native library".to_vec(),
        wat::parse_str("(module)").unwrap(),
    ] {
        assert_eq!(
            RuntimePlugin::new(manifest(), bytes, vec![])
                .await
                .err()
                .unwrap()
                .code,
            "invalid_component"
        );
    }
    let mut changed = manifest();
    changed.version = "2.0.0".into();
    let plugin = RuntimePlugin::new(changed, wat::parse_str(source()).unwrap(), vec![])
        .await
        .unwrap();
    assert_eq!(plugin.verify().await.unwrap_err().code, "manifest_mismatch");
}
#[tokio::test]
async fn infinite_guest_exhausts_fuel_and_memory_growth_traps() {
    for replacement in [
        "(loop br 0) i32.const 16",
        "i32.const 4096 memory.grow drop i32.const 16",
    ] {
        let text = source().replace(
            "(result i32) i32.const 16)",
            &format!("(result i32) {replacement})"),
        );
        let plugin = RuntimePlugin::new(manifest(), wat::parse_str(text).unwrap(), vec![])
            .await
            .unwrap();
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            plugin.execute_runtime(request(), &context()),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, "plugin_trap");
    }
}
#[tokio::test]
async fn canceled_and_expired_calls_stop() {
    let plugin = RuntimePlugin::new(manifest(), wat::parse_str(source()).unwrap(), vec![])
        .await
        .unwrap();
    let context = context();
    context.cancel.cancel();
    assert_eq!(
        plugin
            .execute_runtime(request(), &context)
            .await
            .unwrap_err()
            .code,
        "canceled"
    );
    let mut req = request();
    req.deadline = Utc::now() - TimeDelta::seconds(1);
    assert!(plugin.execute_runtime(req, &self::context()).await.is_err());
}

#[tokio::test]
async fn active_guest_stops_after_its_deadline_or_a_new_cancellation_request() {
    let text = source().replace(
        "(result i32) i32.const 16)",
        "(result i32) (loop br 0) i32.const 16)",
    );
    let plugin = RuntimePlugin::new(manifest(), wat::parse_str(text).unwrap(), vec![])
        .await
        .unwrap();
    for cancel in [false, true] {
        let context = context();
        let mut request = request();
        if cancel {
            let token = context.cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                token.cancel();
            });
        } else {
            request.deadline = Utc::now() + TimeDelta::milliseconds(20);
        }
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            plugin.execute_runtime(request, &context),
        )
        .await
        .expect("running guest stopped within one second")
        .unwrap_err();
        assert!(matches!(
            error.code.as_str(),
            "timeout" | "canceled" | "plugin_trap"
        ));
    }
}
