//! Files embedded into the binary: the maintainer prompt and the vault template.

pub const MAINTAINER_PROMPT: &str = include_str!("../prompts/maintainer.md");

/// `(relative path, contents)` pairs copied by `agent-context init`.
pub const VAULT_TEMPLATE: &[(&str, &str)] = &[
    ("AGENTS.md", include_str!("../templates/vault/AGENTS.md")),
    ("log.md", include_str!("../templates/vault/log.md")),
    (
        "wiki/index.md",
        include_str!("../templates/vault/wiki/index.md"),
    ),
    ("raw/.gitkeep", ""),
    (".agent-context/compiled.txt", ""),
    (".gitignore", include_str!("../templates/vault/gitignore")),
];
