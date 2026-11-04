mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_domain::secrets::{EncryptedSecret, MasterKey};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

async fn body(response: Response, status: StatusCode) -> Value {
    let actual = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(actual, status, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}

async fn ciphertext(fixture: &AuthFixture, id: Uuid) -> EncryptedSecret {
    let (ciphertext, nonce, key_version): (Vec<u8>, Vec<u8>, i32) =
        sqlx::query_as("SELECT ciphertext, nonce, key_version FROM secrets WHERE id=$1")
            .bind(id)
            .fetch_one(&fixture.owner)
            .await
            .unwrap();
    EncryptedSecret {
        ciphertext,
        nonce,
        key_version,
    }
}

#[tokio::test]
#[serial]
async fn admins_manage_write_only_secrets_with_fresh_encryption_and_safe_audit() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let path = format!("/api/v1/orgs/{}/secrets", admin.organization_id);
    let plaintext = "credentials-that-must-never-appear-in-api-read";
    let created = body(
        admin
            .post_json(
                &path,
                &json!({"name":"  Production token  ","value":plaintext}).to_string(),
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["name"], "Production token");
    assert_eq!(created.as_object().unwrap().len(), 4);
    assert!(!created.to_string().contains(plaintext));
    let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let before = ciphertext(&admin, id).await;
    assert!(
        !before
            .ciphertext
            .windows(plaintext.len())
            .any(|window| window == plaintext.as_bytes())
    );
    let key = MasterKey::from_bytes([7; 32]);
    assert_eq!(
        key.decrypt(admin.organization_id, id, &before).unwrap(),
        plaintext
    );
    let listed = body(admin.get(&path).await, StatusCode::OK).await;
    assert_eq!(listed["items"], json!([created]));
    let detail = format!("{path}/{id}");
    assert_eq!(
        admin.get(&detail).await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    body(
        admin
            .request_json(
                Method::PUT,
                &detail,
                &json!({"name":"Production token","value":plaintext}).to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    assert_ne!(before.nonce, ciphertext(&admin, id).await.nonce);
    assert_eq!(admin.delete(&detail).await.status(), StatusCode::NO_CONTENT);
    let audit = body(
        admin
            .get(&format!(
                "/api/v1/orgs/{}/audit-events",
                admin.organization_id
            ))
            .await,
        StatusCode::OK,
    )
    .await;
    assert!(!audit.to_string().contains(plaintext));
    let actions: Vec<_> = audit["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "secret")
        .map(|event| event["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        ["secret.deleted", "secret.updated", "secret.created"]
    );
}

#[tokio::test]
#[serial]
async fn secret_names_are_editor_visible_but_only_admins_mutate_and_tenants_are_isolated() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let outsider = admin.register_another("other@example.com", "Other").await;
    let member = admin
        .register_another("member@example.com", "Personal")
        .await;
    let path = format!("/api/v1/orgs/{}/secrets", admin.organization_id);
    let request = json!({"name":"API key","value":"unique-secret-value"}).to_string();
    let created = body(admin.post_json(&path, &request).await, StatusCode::CREATED).await;
    let detail = format!("{path}/{}", created["id"].as_str().unwrap());
    assert_eq!(outsider.get(&path).await.status(), StatusCode::NOT_FOUND);
    let foreign_detail = format!(
        "/api/v1/orgs/{}/secrets/{}",
        outsider.organization_id,
        created["id"].as_str().unwrap()
    );
    assert_eq!(
        outsider.delete(&foreign_detail).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        outsider
            .request_json(Method::PUT, &foreign_detail, &request)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO memberships(organization_id,user_id,role) VALUES ($1,$2,'editor')")
        .bind(admin.organization_id)
        .bind(member.user_id)
        .execute(&admin.owner)
        .await
        .unwrap();
    assert_eq!(member.get(&path).await.status(), StatusCode::OK);
    assert_eq!(
        member.post_json(&path, &request).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(member.delete(&detail).await.status(), StatusCode::FORBIDDEN);
    for role in ["runner", "viewer"] {
        sqlx::query("UPDATE memberships SET role=$3 WHERE organization_id=$1 AND user_id=$2")
            .bind(admin.organization_id)
            .bind(member.user_id)
            .bind(role)
            .execute(&admin.owner)
            .await
            .unwrap();
        assert_eq!(member.get(&path).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            member
                .request_json(Method::PUT, &detail, &request)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
#[serial]
async fn drafts_and_published_versions_protect_references_and_cross_org_publish_is_rejected() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let outsider = admin.register_another("other@example.com", "Other").await;
    let secrets = format!("/api/v1/orgs/{}/secrets", admin.organization_id);
    let secret = body(
        admin
            .post_json(&secrets, r#"{"name":"Region","value":"ap-southeast-1"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let foreign = body(
        outsider
            .post_json(
                &format!("/api/v1/orgs/{}/secrets", outsider.organization_id),
                r#"{"name":"Other","value":"cross-tenant-value"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let workflow = body(
        admin
            .post_json(
                &workflows,
                r#"{"name":"Secret flow","blueprintId":"00000000-0000-7000-8000-000000000101"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let path = format!("{workflows}/{}", workflow["id"].as_str().unwrap());
    let mut graph = workflow["draft"]["graph"].clone();
    graph["nodes"][0]["inputs"]["region"] = json!({"kind":"secret","secretId":secret["id"]});
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{path}/draft"),
                &json!({"revision":1,"graph":graph}).to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    let detail = format!("{secrets}/{}", secret["id"].as_str().unwrap());
    assert_eq!(
        body(admin.delete(&detail).await, StatusCode::CONFLICT).await["code"],
        "secret_in_use"
    );
    body(
        admin
            .post_json(&format!("{path}/publish"), r#"{"revision":2}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    // The old published version protects its secret after the draft is changed.
    graph["nodes"][0]["inputs"]["region"] = json!({"kind":"secret","secretId":foreign["id"]});
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{path}/draft"),
                &json!({"revision":2,"graph":graph}).to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    let rejected = body(
        admin
            .post_json(&format!("{path}/publish"), r#"{"revision":3}"#)
            .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert!(
        rejected["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "secret_not_found")
    );
    assert_eq!(admin.delete(&detail).await.status(), StatusCode::CONFLICT);
}

#[tokio::test]
#[serial]
async fn definition_secret_defaults_require_valid_opaque_references_from_this_organization() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let outsider = admin.register_another("other@example.com", "Other").await;
    let own = body(
        admin
            .post_json(
                &format!("/api/v1/orgs/{}/secrets", admin.organization_id),
                r#"{"name":"Own","value":"own-secret"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let foreign = body(
        outsider
            .post_json(
                &format!("/api/v1/orgs/{}/secrets", outsider.organization_id),
                r#"{"name":"Foreign","value":"foreign-secret"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let path = format!("/api/v1/orgs/{}/node-definitions", admin.organization_id);
    let mut definition = json!({
        "key":"acme.secret-default", "displayName":"Secret default", "category":"Testing",
        "inputs":[],"outputs":[],
        "configSchema":{"type":"object","properties":{"token":{"type":"object"}}},
        "uiSchema":{"token":{"widget":"secretReference"}},
        "defaults":{"token":{"kind":"secret","secretId":own["id"]}},
        "executor":{"key":"mock","version":"1.0.0"}
    });
    body(
        admin.post_json(&path, &definition.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    definition["defaults"]["token"]["secretId"] = foreign["id"].clone();
    let rejected = body(
        admin.post_json(&path, &definition.to_string()).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert!(
        rejected["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "secret_not_found" && d["pointer"] == "/defaults/token/secretId")
    );
    definition["defaults"]["token"] = json!({"plaintext":"must-not-be-a-secret-reference"});
    let rejected = body(
        admin.post_json(&path, &definition.to_string()).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert!(
        rejected["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "invalid_secret_reference" && d["pointer"] == "/defaults/token")
    );
}

#[tokio::test]
#[serial]
async fn publishing_checks_every_node_after_more_than_two_hundred_valid_secret_references() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let base = format!("/api/v1/orgs/{}", admin.organization_id);
    let secret = body(
        admin
            .post_json(
                &format!("{base}/secrets"),
                r#"{"name":"Region","value":"ap-southeast-1"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = body(admin.post_json(&format!("{base}/workflows"), r#"{"name":"Large secret flow","blueprintId":"00000000-0000-7000-8000-000000000101"}"#).await, StatusCode::CREATED).await;
    let path = format!("{base}/workflows/{}", workflow["id"].as_str().unwrap());
    let mut graph = workflow["draft"]["graph"].clone();
    let template = graph["nodes"][0].clone();
    graph["edges"] = json!([]);
    graph["nodes"] = Value::Array(
        (0..260)
            .map(|index| {
                let mut node = template.clone();
                node["id"] = json!(format!("n{index}"));
                node["inputs"]["region"] = json!({"kind":"secret","secretId":secret["id"]});
                node
            })
            .collect(),
    );
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{path}/draft"),
                &json!({"revision":1,"graph":graph}).to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    body(
        admin
            .post_json(&format!("{path}/publish"), r#"{"revision":2}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    graph["nodes"][259]["displayName"] = json!("");
    body(
        admin
            .request_json(
                Method::PUT,
                &format!("{path}/draft"),
                &json!({"revision":2,"graph":graph}).to_string(),
            )
            .await,
        StatusCode::OK,
    )
    .await;
    let rejected = body(
        admin
            .post_json(&format!("{path}/publish"), r#"{"revision":3}"#)
            .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(rejected["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(rejected["diagnostics"][0]["code"], "invalid_display_name");
    assert_eq!(
        rejected["diagnostics"][0]["pointer"],
        "/nodes/259/displayName"
    );
}
