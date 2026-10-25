use corqty_domain::{
    account::{NormalizedEmail, OrganizationName, validate_password},
    rbac::{Capability, Role},
};

#[test]
fn normalizes_email_and_organization_name() {
    let email = NormalizedEmail::parse("  Admin@Example.COM ").unwrap();
    let name = OrganizationName::parse("  Acme Cloud  ").unwrap();

    assert_eq!(email.as_str(), "admin@example.com");
    assert_eq!(name.as_str(), "Acme Cloud");
}

#[test]
fn rejects_invalid_account_values() {
    assert!(NormalizedEmail::parse("not-an-email").is_err());
    assert!(OrganizationName::parse(" ").is_err());
    assert!(validate_password("short").is_err());
    assert!(validate_password(&"x".repeat(129)).is_err());
}

#[test]
fn fixed_roles_match_the_approved_permission_matrix() {
    let roles = [Role::Admin, Role::Editor, Role::Runner, Role::Viewer];
    for (capability, expected) in [
        (Capability::ViewOrganization, [true, true, true, true]),
        (Capability::EditWorkflows, [true, true, false, false]),
        (Capability::PublishWorkflows, [true, true, false, false]),
        (Capability::RunWorkflows, [true, true, true, false]),
        (Capability::ManageMembers, [true, false, false, false]),
        (Capability::ManageServiceTokens, [true, false, false, false]),
        (Capability::ManageSecrets, [true, false, false, false]),
        (Capability::ManagePlugins, [true, false, false, false]),
        (Capability::EnableShellExecutor, [true, false, false, false]),
    ] {
        for (role, allowed) in roles.into_iter().zip(expected) {
            assert_eq!(role.allows(capability), allowed, "{role}: {capability:?}");
        }
    }
}

#[test]
fn roles_list_their_capabilities_in_matrix_order() {
    assert_eq!(Role::Admin.capabilities(), Capability::ALL);
    assert_eq!(
        Role::Editor.capabilities(),
        [
            Capability::ViewOrganization,
            Capability::EditWorkflows,
            Capability::PublishWorkflows,
            Capability::RunWorkflows,
        ]
    );
    assert_eq!(
        Role::Runner.capabilities(),
        [Capability::ViewOrganization, Capability::RunWorkflows]
    );
    assert_eq!(Role::Viewer.capabilities(), [Capability::ViewOrganization]);
}

#[test]
fn roles_round_trip_as_snake_case() {
    for (text, role) in [
        ("admin", Role::Admin),
        ("editor", Role::Editor),
        ("runner", Role::Runner),
        ("viewer", Role::Viewer),
    ] {
        assert_eq!(text.parse::<Role>().unwrap(), role);
        assert_eq!(role.as_str(), text);
    }
    assert!("owner".parse::<Role>().is_err());
}
