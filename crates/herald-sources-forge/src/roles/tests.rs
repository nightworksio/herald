use super::*;

#[test]
fn gitlab_levels_make_each_role_from_developer_up() {
    let roles: Vec<Option<Permission>> = [0, 10, 20, 29, 30, 39, 40, 49, 50, 60]
        .into_iter()
        .map(gitlab_role)
        .collect();
    let (write, maintain, admin) = (
        Some(Permission::Write),
        Some(Permission::Maintain),
        Some(Permission::Admin),
    );
    assert_eq!(
        roles,
        [
            None, None, None, None, write, write, maintain, maintain, admin, admin
        ]
    );
}

#[test]
fn github_permissions_make_the_highest_role_they_hold() {
    let role = |body: &str| {
        serde_json::from_str::<GithubPermissions>(body)
            .unwrap()
            .role()
    };
    assert_eq!(
        role(r#"{"admin":true,"push":true}"#),
        Some(Permission::Admin)
    );
    assert_eq!(
        role(r#"{"maintain":true,"push":true}"#),
        Some(Permission::Maintain)
    );
    assert_eq!(
        role(r#"{"push":true,"triage":true}"#),
        Some(Permission::Write)
    );
    assert_eq!(role(r#"{"pull":true,"triage":true}"#), None);
}

#[test]
fn forgejo_permissions_make_write_or_admin() {
    let roles: Vec<Option<Permission>> = ["owner", "admin", "write", "read", "none", "Write"]
        .into_iter()
        .map(forgejo_role)
        .collect();
    assert_eq!(
        roles,
        [
            Some(Permission::Admin),
            Some(Permission::Admin),
            Some(Permission::Write),
            None,
            None,
            None
        ]
    );
}
