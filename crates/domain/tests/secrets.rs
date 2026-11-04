use base64::{Engine, engine::general_purpose::STANDARD};
use corqty_domain::secrets::{MasterKey, Redactor, SecretCryptoError};
use serde_json::json;
use uuid::Uuid;

#[test]
fn fresh_nonces_and_authenticated_tenant_record_and_key_version() {
    let key = MasterKey::from_bytes([42; 32]);
    let (org, id) = (Uuid::new_v4(), Uuid::new_v4());
    let first = key.encrypt(org, id, "super-secret-value").unwrap();
    let second = key.encrypt(org, id, "super-secret-value").unwrap();
    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.ciphertext, second.ciphertext);
    assert_eq!(key.decrypt(org, id, &first).unwrap(), "super-secret-value");
    assert!(key.decrypt(Uuid::new_v4(), id, &first).is_err());
    assert!(key.decrypt(org, Uuid::new_v4(), &first).is_err());
    assert!(
        MasterKey::from_bytes([43; 32])
            .decrypt(org, id, &first)
            .is_err()
    );
    let mut tampered = first.clone();
    tampered.ciphertext[0] ^= 1;
    assert!(key.decrypt(org, id, &tampered).is_err());
    tampered.key_version = 2;
    assert!(matches!(
        key.decrypt(org, id, &tampered),
        Err(SecretCryptoError::KeyVersion)
    ));
    tampered.key_version = 1;
    tampered.nonce.clear();
    assert!(key.decrypt(org, id, &tampered).is_err());
    assert!(!format!("{key:?}").contains("42"));
}

#[test]
fn master_key_is_strictly_32_bytes_base64() {
    assert!(MasterKey::from_base64("not-base64").is_err());
    assert!(MasterKey::from_base64("c2hvcnQ=").is_err());
    assert!(MasterKey::from_base64(&STANDARD.encode([7; 32])).is_ok());
}

#[test]
fn exact_values_are_redacted_in_nested_keys_strings_and_scalar_values() {
    let redactor = Redactor::new(["secret".into(), "secret-long".into(), "42".into()]);
    assert_eq!(
        redactor.text("x secret-long and secret"),
        "x [REDACTED] and [REDACTED]"
    );
    let value = json!({"secret-key":["x secret-long", 42, "safe"], "ok": true});
    assert!(redactor.contains_json(&value));
    assert_eq!(
        redactor.json(&value),
        json!({"[REDACTED]-key":["x [REDACTED]", "[REDACTED]", "safe"], "ok":true})
    );
}

#[test]
fn secret_split_across_every_possible_chunk_and_flush_boundary_is_hidden() {
    let secret = "secret-🔐-token";
    let redactor = Redactor::new([secret.to_owned()]);
    for (boundary, _) in secret.char_indices() {
        let mut pending = format!("before {}", &secret[..boundary]);
        let first = redactor.stream_prefix(&mut pending, false);
        pending.push_str(&secret[boundary..]);
        pending.push_str(" after");
        let second = redactor.stream_prefix(&mut pending, true);
        assert_eq!(format!("{first}{second}"), "before [REDACTED] after");
    }
}
