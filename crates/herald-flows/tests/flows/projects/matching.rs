use super::*;

/// The projects route taking the owner's members only where an override
/// names it, narrowed by a match.
fn matched() -> Config {
    let text = config_text()
        .replace(
            "sources = [\"org\", \"gate\", \"ci\"]",
            "sources = [\"gate\", \"ci\"]\nmatch = { projects = [\"c*\", \"g*\", \"php-*\", \"Upper\"] }",
        )
        .replace(
            "[sources.gate]",
            "[sources.org.repositories.\"c*\"]\nroutes = [\"projects\"]\n[sources.org.repositories.\"Upper\"]\nroutes = [\"projects\"]\n[sources.gate]",
        );
    configured(&text)
}

#[test]
fn an_actor_is_served_only_for_the_projects_a_route_s_match_and_its_members_overrides_take() {
    let world = world_of(matched());
    for (name, status) in [
        ("cli", 200),
        ("gh_tools", 404),
        ("php-mutation-gate", 200),
        ("herald", 404),
        ("secret", 404),
    ] {
        let path = format!("/users/{name}");
        assert_eq!(world.answer(&get(&path)).reply.status, status, "{name}");
    }
}
