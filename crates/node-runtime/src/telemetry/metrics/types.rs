//! Only these bounded, content-free dimensions may enter metric payloads.
use sailry_protocol::{
    Authentication, ErrorCode, WorkMode,
    conversation::{ModelApi, Provider, presets},
    search::Driver,
};
use serde::Serialize;
use std::{sync::OnceLock, time::Duration};

pub(crate) enum Observation {
    Launch,
    Turn {
        result: TurnResult,
        mode: WorkMode,
        subagent: bool,
        error: Option<ErrorCode>,
    },
    Tool {
        tool: Tool,
        result: ToolResult,
        error: Option<ErrorCode>,
        elapsed: Duration,
    },
    Model {
        model: Model,
        result: RequestResult,
        status: StatusClass,
        error: Option<ErrorCode>,
        retries: Option<u64>,
        tokens: Tokens,
    },
    Search {
        driver: Option<Driver>,
        result: RequestResult,
        error: Option<ErrorCode>,
        elapsed: Duration,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TurnResult {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolResult {
    Success,
    Error,
    Denied,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestResult {
    Success,
    Error,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum StatusClass {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "1xx")]
    Informational,
    #[serde(rename = "2xx")]
    Success,
    #[serde(rename = "3xx")]
    Redirect,
    #[serde(rename = "4xx")]
    ClientError,
    #[serde(rename = "5xx")]
    ServerError,
}

impl From<Option<u16>> for StatusClass {
    fn from(status: Option<u16>) -> Self {
        match status.map(|status| status / 100) {
            Some(1) => Self::Informational,
            Some(2) => Self::Success,
            Some(3) => Self::Redirect,
            Some(4) => Self::ClientError,
            Some(5) => Self::ServerError,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Model {
    api: ModelApi,
    authentication: Authentication,
    model: String,
}

impl Model {
    pub(crate) fn capture(provider: &Provider, model: &str) -> Self {
        static ALLOWED: OnceLock<Vec<(String, Vec<String>)>> = OnceLock::new();
        let allowed = ALLOWED.get_or_init(|| {
            crate::providers::presets::bundled()
                .builtin
                .into_iter()
                .filter_map(|preset| match preset.models {
                    presets::Models::Static { ids } => Some((preset.id, ids)),
                    presets::Models::Discover => None,
                })
                .collect()
        });
        Self::selected(provider, model, allowed)
    }

    pub(super) fn selected(
        provider: &Provider,
        model: &str,
        allowed: &[(String, Vec<String>)],
    ) -> Self {
        let builtin = provider.preset.as_ref().is_some_and(|preset| {
            matches!(preset.source, presets::Source::Builtin)
                && allowed.iter().any(|(id, models)| {
                    id == &preset.id && models.iter().any(|allowed| allowed == model)
                })
        });
        Self {
            api: provider.api,
            authentication: provider.authentication,
            model: if builtin { model } else { "custom" }.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Tool(Source);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
enum Source {
    Builtin {
        name: &'static str,
    },
    Package {
        package: String,
        version: String,
        name: String,
    },
    Mcp,
}

impl Tool {
    pub(crate) fn mcp() -> Self {
        Self(Source::Mcp)
    }

    pub(crate) fn capture(name: &str, packages: &[sailry_protocol::plugin::Info]) -> Option<Self> {
        if name.starts_with("mcp_") {
            return Some(Self::mcp());
        }
        Self::builtin(name).or_else(|| {
            packages.iter().find_map(|package| {
                package.extension.as_ref()?.tools.iter().find_map(|tool| {
                    (tool.valid()
                        && tool.server.is_none()
                        && crate::plugins::tools::alias(&package.summary.name, &tool.name) == name)
                        .then(|| Self::package(package, tool))
                        .flatten()
                })
            })
        })
    }

    pub(crate) fn builtin(name: &str) -> Option<Self> {
        let name = match name {
            "ask_user" => "ask_user",
            "plan_review" => "plan_review",
            "list_sessions" => "list_sessions",
            "read_session" => "read_session",
            "search_session" => "search_session",
            "read_tool_output" => "read_tool_output",
            "load_skill" => "load_skill",
            "read_skill_resource" => "read_skill_resource",
            "tool_search" => "tool_search",
            "manage_plugins" => "manage_plugins",
            "manage_mcp" => "manage_mcp",
            "manage_skills" => "manage_skills",
            _ => return None,
        };
        Some(Self(Source::Builtin { name }))
    }

    fn package(
        package: &sailry_protocol::plugin::Info,
        declaration: &sailry_protocol::tool::Declaration,
    ) -> Option<Self> {
        let version = package.summary.version.as_deref()?;
        if package.summary.name.len() > 128 || version.len() > 64 || declaration.name.len() > 128 {
            return None;
        }
        package
            .extension
            .as_ref()?
            .tools
            .iter()
            .any(|tool| tool.name == declaration.name)
            .then(|| {
                Self(Source::Package {
                    package: package.summary.name.clone(),
                    version: version.into(),
                    name: declaration.name.clone(),
                })
            })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Table {
    Launches,
    Turns,
    Tools,
    Models,
    Searches,
}

impl Table {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Launches => "app_launches",
            Self::Turns => "turns",
            Self::Tools => "tool_calls",
            Self::Models => "model_requests",
            Self::Searches => "search_requests",
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Dimensions {
    Launch {},
    Turn {
        result: TurnResult,
        mode: WorkMode,
        subagent: bool,
        error_code: Option<ErrorCode>,
    },
    Tool {
        tool: Tool,
        result: ToolResult,
        error_code: Option<ErrorCode>,
    },
    Model {
        #[serde(flatten)]
        model: Model,
        result: RequestResult,
        http_status_class: StatusClass,
        error_code: Option<ErrorCode>,
        retries: Option<u64>,
    },
    Search {
        driver: Option<Driver>,
        result: RequestResult,
        error_code: Option<ErrorCode>,
    },
}

#[derive(Default, Serialize)]
pub(super) struct Counts {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_us_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_us_max: Option<u64>,
    #[serde(flatten)]
    pub tokens: Option<Tokens>,
}

impl Observation {
    pub(super) fn sample(self) -> (Table, Dimensions, Counts) {
        let mut counts = Counts {
            count: 1,
            ..Default::default()
        };
        let (table, dimensions) = match self {
            Self::Launch => (Table::Launches, Dimensions::Launch {}),
            Self::Turn {
                result,
                mode,
                subagent,
                error,
            } => (
                Table::Turns,
                Dimensions::Turn {
                    result,
                    mode,
                    subagent,
                    error_code: error,
                },
            ),
            Self::Tool {
                tool,
                result,
                error,
                elapsed,
            } => {
                counts.elapsed_us_total = Some(micros(elapsed));
                counts.elapsed_us_max = counts.elapsed_us_total;
                (
                    Table::Tools,
                    Dimensions::Tool {
                        tool,
                        result,
                        error_code: error,
                    },
                )
            }
            Self::Model {
                model,
                result,
                status,
                error,
                retries,
                tokens,
            } => {
                counts.tokens = Some(tokens);
                (
                    Table::Models,
                    Dimensions::Model {
                        model,
                        result,
                        http_status_class: status,
                        error_code: error,
                        retries,
                    },
                )
            }
            Self::Search {
                driver,
                result,
                error,
                elapsed,
            } => {
                counts.elapsed_us_total = Some(micros(elapsed));
                (
                    Table::Searches,
                    Dimensions::Search {
                        driver,
                        result,
                        error_code: error,
                    },
                )
            }
        };
        (table, dimensions, counts)
    }
}

impl Counts {
    pub(super) fn add(&mut self, value: Self) {
        self.count = self.count.saturating_add(value.count);
        if let (Some(total), Some(value)) = (&mut self.elapsed_us_total, value.elapsed_us_total) {
            *total = total.saturating_add(value);
        }
        if let (Some(total), Some(value)) = (&mut self.elapsed_us_max, value.elapsed_us_max) {
            *total = (*total).max(value);
        }
        if let (Some(total), Some(value)) = (&mut self.tokens, value.tokens) {
            total.input = total.input.saturating_add(value.input);
            total.output = total.output.saturating_add(value.output);
            total.cache_read = total.cache_read.saturating_add(value.cache_read);
            total.cache_write = total.cache_write.saturating_add(value.cache_write);
        }
    }
}

fn micros(value: Duration) -> u64 {
    u64::try_from(value.as_micros()).unwrap_or(u64::MAX)
}
