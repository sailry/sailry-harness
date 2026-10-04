//! Plugin assistant declarations bind ordinary Node-owned sessions.
use super::Reference;
use crate::{ProjectId, connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub id: String,
    pub resource: Resource,
    pub tools: Vec<Tool>,
    /// Static package-owned context; controllers cannot replace it at mount time.
    pub context: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    Plugin,
    Workspace,
    Database,
    Ssh,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Tool {
    Model {
        capability: super::model_tools::Capability,
    },
    Builtin {
        name: String,
    },
    /// A tool from this declaring package; an MCP tool also identifies its server.
    Plugin {
        name: String,
        server: Option<String>,
    },
    /// One declared operation or script tool from another installed package.
    Package {
        package: String,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub package: Reference,
    pub id: String,
}

impl Declaration {
    pub fn valid(&self) -> bool {
        valid_id(&self.id)
            && self.context.len() <= 16 * 1024
            && !self.context.contains('\0')
            && self.tools.len() <= 128
            && self
                .tools
                .iter()
                .enumerate()
                .all(|(index, tool)| tool.valid() && !self.tools[..index].contains(tool))
    }
}

impl Resource {
    pub fn matches(
        self,
        project: Option<ProjectId>,
        resource: Option<connection::Resource>,
    ) -> bool {
        match self {
            Self::Plugin => project.is_none() && resource.is_none(),
            Self::Workspace => project.is_some() && resource.is_none(),
            Self::Database => {
                project.is_none() && matches!(resource, Some(connection::Resource::Database(_)))
            }
            Self::Ssh => {
                project.is_none() && matches!(resource, Some(connection::Resource::Ssh(_)))
            }
        }
    }
}

impl Tool {
    pub fn valid(&self) -> bool {
        fn name(value: &str) -> bool {
            !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        }
        match self {
            Self::Model { .. } => true,
            Self::Builtin { name: value } => name(value),
            Self::Plugin {
                name: value,
                server,
            } => name(value) && server.as_deref().is_none_or(name),
            Self::Package {
                package,
                name: value,
            } => {
                name(value)
                    && !package.is_empty()
                    && package.len() <= 64
                    && package.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'.')
                    })
                    && package
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && package
                        .as_bytes()
                        .last()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && !package.contains("--")
                    && !package.contains("..")
            }
        }
    }
}

pub fn valid(declarations: &[Declaration]) -> bool {
    declarations.len() <= 16
        && declarations.iter().enumerate().all(|(index, declaration)| {
            declaration.valid()
                && !declarations[..index]
                    .iter()
                    .any(|other| other.id == declaration.id)
        })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds() {
        let mut declaration = Declaration {
            id: "assistant".into(),
            resource: Resource::Plugin,
            tools: vec![Tool::Plugin {
                name: "read_note".into(),
                server: None,
            }],
            context: "Help with the plugin's notes".into(),
        };
        assert!(valid(&[declaration.clone()]));
        assert!(!valid(&[declaration.clone(), declaration.clone()]));
        declaration.tools.push(declaration.tools[0].clone());
        assert!(!declaration.valid());
        declaration.tools.clear();
        declaration.context = "x".repeat(16 * 1024 + 1);
        assert!(!declaration.valid());
        declaration.context.clear();
        declaration.id = "../other".into();
        assert!(!declaration.valid());
    }

    #[test]
    fn scopes() {
        let database = connection::Resource::Database(crate::DatabaseId::new());
        assert!(Resource::Plugin.matches(None, None));
        assert!(!Resource::Plugin.matches(None, Some(database)));
        assert!(Resource::Database.matches(None, Some(database)));
        assert!(!Resource::Ssh.matches(None, Some(database)));
        assert!(!Resource::Workspace.matches(None, None));
        assert!(Resource::Workspace.matches(Some(ProjectId::new()), None));
    }

    #[test]
    fn package_tools_are_explicit() {
        let value = serde_json::json!({"kind":"package","package":"files","name":"read_file"});
        let tool: Tool = serde_json::from_value(value.clone()).unwrap();
        assert!(tool.valid());
        for package in ["", "../files", "Files", "files--tools", "files."] {
            let mut invalid = value.clone();
            invalid["package"] = package.into();
            assert!(!serde_json::from_value::<Tool>(invalid).unwrap().valid());
        }
        let mut mcp = value;
        mcp["server"] = "files".into();
        assert!(serde_json::from_value::<Tool>(mcp).is_err());
    }
}
