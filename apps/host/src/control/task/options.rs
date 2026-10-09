use super::{Target, Task};
use sailry_protocol::*;
use std::{ffi::OsString, io::Read, path::PathBuf};

const USAGE: &str = "Usage: sailry submit (--session <id> | --worktree <id>) (--prompt <text> | --prompt-file <path> | --stdin) [--request-id <id>] [--expected-revision <number>] [--config <JSON file>] [--title <text>] [--no-wait] [--timeout <seconds>] [--data-dir <profile>]\n       sailry wait --session <id> --turn <id> [--timeout <seconds>] [--data-dir <profile>]\nWrites one JSON result. Exit codes: 0 admitted/completed, 1 failed/stopped, 3 approval/input/paused, 4 uncertain, 124 timeout, 130 observation cancelled. Waiting never grants approvals or stops execution.";

pub(super) struct Options {
    pub profile: PathBuf,
    pub task: Task,
    pub wait: bool,
}

enum Input {
    Text(String),
    File(PathBuf),
    Stdin,
}

pub(super) fn parse(
    arguments: impl Iterator<Item = OsString>,
) -> Result<Option<Options>, Box<dyn std::error::Error>> {
    let mut arguments = arguments;
    let command = arguments.next().ok_or(USAGE)?;
    if command != "submit" && command != "wait" {
        return Err(USAGE.into());
    }
    let mut profile = None;
    let mut session: Option<SessionId> = None;
    let mut worktree: Option<WorktreeId> = None;
    let mut turn: Option<TurnId> = None;
    let mut request: Option<RequestId> = None;
    let mut revision = None;
    let mut config = None;
    let mut input = None;
    let mut title = None;
    let mut timeout = None;
    let mut wait = true;
    while let Some(argument) = arguments.next() {
        if argument == "--help" || argument == "-h" {
            println!("{USAGE}");
            return Ok(None);
        }
        if argument == "--stdin" && input.is_none() && command == "submit" {
            input = Some(Input::Stdin);
            continue;
        }
        if argument == "--no-wait" && wait && command == "submit" {
            wait = false;
            continue;
        }
        let value = arguments.next().ok_or(USAGE)?;
        if argument == "--data-dir" && profile.is_none() {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err("--data-dir must be absolute".into());
            }
            profile = Some(path);
        } else if argument == "--prompt-file" && input.is_none() && command == "submit" {
            input = Some(Input::File(PathBuf::from(value)));
        } else if argument == "--config" && config.is_none() && command == "submit" {
            config = Some(PathBuf::from(value));
        } else {
            let value = value
                .into_string()
                .map_err(|_| "task argument must be UTF-8")?;
            if argument == "--session" && session.is_none() {
                session = Some(value.parse()?);
            } else if argument == "--worktree" && worktree.is_none() && command == "submit" {
                worktree = Some(value.parse()?);
            } else if argument == "--turn" && turn.is_none() && command == "wait" {
                turn = Some(value.parse()?);
            } else if argument == "--request-id" && request.is_none() && command == "submit" {
                request = Some(value.parse()?);
            } else if argument == "--expected-revision" && revision.is_none() && command == "submit"
            {
                revision = Some(value.parse::<u64>()?);
            } else if argument == "--timeout" && timeout.is_none() {
                timeout = Some(value.parse::<u64>()?);
            } else if argument == "--title" && title.is_none() && command == "submit" {
                title = Some(value);
            } else if argument == "--prompt" && input.is_none() && command == "submit" {
                input = Some(Input::Text(value));
            } else {
                return Err(USAGE.into());
            }
        }
    }
    let timeout = timeout.unwrap_or(600);
    if !(1..=86400).contains(&timeout) {
        return Err("--timeout must be between 1 and 86400 seconds".into());
    }
    let task = if command == "wait" {
        Task::Wait {
            version: 1,
            session: session.ok_or(USAGE)?,
            turn: turn.ok_or(USAGE)?,
            timeout,
        }
    } else {
        if session.is_some() == worktree.is_some() {
            return Err("choose one session or worktree".into());
        }
        if session.is_some() && (config.is_some() || title.is_some())
            || worktree.is_some() && revision.is_some()
        {
            return Err("configuration/title require a new worktree task; expected revision requires a session".into());
        }
        let text = match input.ok_or(USAGE)? {
            Input::Text(text) => text,
            Input::File(path) => read(std::fs::File::open(path)?)?,
            Input::Stdin => read(std::io::stdin().lock())?,
        };
        if text.trim().is_empty() || text.len() > super::MAX_REQUEST {
            return Err("task text is empty or too large".into());
        }
        let target = match session {
            Some(session) => Target::Session {
                session,
                expected_revision: revision,
            },
            None => Target::Worktree {
                worktree: worktree.unwrap(),
                config: config
                    .map(|path| -> Result<_, Box<dyn std::error::Error>> {
                        Ok(Box::new(serde_json::from_str::<SessionConfig>(&read(
                            std::fs::File::open(path)?,
                        )?)?))
                    })
                    .transpose()?,
            },
        };
        let title = if session.is_some() {
            String::new()
        } else {
            title.unwrap_or_else(|| text.lines().next().unwrap_or("").chars().take(80).collect())
        };
        Task::Submit {
            version: 1,
            request: request.unwrap_or_default(),
            target,
            message: text.into(),
            title,
            wait,
            timeout,
        }
    };
    let profile = match profile {
        Some(profile) => profile,
        None => sailry_node_runtime::default_data_dir()?,
    };
    Ok(Some(Options {
        profile,
        task,
        wait,
    }))
}

fn read(reader: impl Read) -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    reader
        .take((super::MAX_REQUEST + 1) as u64)
        .read_to_string(&mut text)?;
    if text.len() > super::MAX_REQUEST {
        return Err("task input exceeds its limit".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
