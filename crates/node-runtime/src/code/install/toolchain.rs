use super::{
    archive,
    plan::{Kind, Plan},
};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

pub(super) async fn install(
    plan: &Plan,
    stage: &Path,
    final_path: &Path,
    environment: &crate::process::Environment,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let stage = &stage.canonicalize().map_err(invalid)?;
    let Kind::Toolchain {
        manager,
        executable,
        name,
        extra,
    } = &plan.kind
    else {
        unreachable!("toolchain plan expected")
    };
    let version = &plan.consent.version;
    let mut overrides = BTreeMap::new();
    let mut entries = BTreeMap::<String, String>::new();
    match manager.as_str() {
        "npm" => {
            let mut args = vec![
                "install".into(),
                "--no-audit".into(),
                "--no-fund".into(),
                "--prefix".into(),
                stage.to_string_lossy().into_owned(),
                format!("{name}@{version}"),
            ];
            args.extend(extra.clone());
            run(executable, &args, stage, environment, &overrides, stop).await?;
        }
        "pypi" => {
            let venv = stage.join(".venv");
            run(
                executable,
                &[
                    "-m".into(),
                    "venv".into(),
                    venv.to_string_lossy().into_owned(),
                ],
                stage,
                environment,
                &overrides,
                stop,
            )
            .await?;
            let python = venv.join(if cfg!(windows) {
                "Scripts/python.exe"
            } else {
                "bin/python"
            });
            run(
                &python,
                &[
                    "-m".into(),
                    "pip".into(),
                    "install".into(),
                    "--disable-pip-version-check".into(),
                    "--no-input".into(),
                    format!("{name}=={version}"),
                ],
                stage,
                environment,
                &overrides,
                stop,
            )
            .await?;
            let metadata = stage.join("console-scripts.json");
            let script = r#"import importlib.metadata, json, pathlib, sys
entries = {
    entry.name: entry.value
    for distribution in importlib.metadata.distributions()
    for entry in distribution.entry_points
    if entry.group == 'console_scripts'
}
pathlib.Path(sys.argv[1]).write_text(json.dumps(entries))
"#;
            run(
                &python,
                &[
                    "-c".into(),
                    script.into(),
                    metadata.to_string_lossy().into_owned(),
                ],
                stage,
                environment,
                &overrides,
                stop,
            )
            .await?;
            entries = serde_json::from_slice(&std::fs::read(metadata).map_err(invalid)?)
                .map_err(invalid)?;
        }
        "cargo" => {
            run(
                executable,
                &[
                    "install".into(),
                    "--locked".into(),
                    "--root".into(),
                    stage.to_string_lossy().into_owned(),
                    name.clone(),
                    "--version".into(),
                    version.clone(),
                ],
                stage,
                environment,
                &overrides,
                stop,
            )
            .await?;
        }
        "golang" => {
            std::fs::create_dir_all(stage.join("bin")).map_err(invalid)?;
            overrides.insert("GOBIN".into(), stage.join("bin").into_os_string());
            run(
                executable,
                &["install".into(), format!("{name}@{version}")],
                stage,
                environment,
                &overrides,
                stop,
            )
            .await?;
        }
        _ => unreachable!("supported toolchain expected"),
    }
    for (alias, binary) in &plan.binaries {
        let source = match manager.as_str() {
            "npm" => stage.join("node_modules/.bin").join(binary),
            "pypi" => stage
                .join(if cfg!(windows) {
                    ".venv/Scripts"
                } else {
                    ".venv/bin"
                })
                .join(binary),
            _ => stage.join("bin").join(binary),
        };
        let source = if cfg!(windows) && !source.exists() {
            if source.with_extension("cmd").exists() {
                source.with_extension("cmd")
            } else {
                source.with_extension("exe")
            }
        } else {
            source
        };
        if !source.is_file() {
            return Err(Fault::invalid(
                "installed language package did not provide its declared binary",
            ));
        }
        let resolved = source.canonicalize().map_err(invalid)?;
        if !resolved.starts_with(stage) {
            return Err(Fault::invalid(
                "installed language binary points outside its package",
            ));
        }
        std::fs::create_dir_all(stage.join("bin")).map_err(invalid)?;
        let relative = source.strip_prefix(stage).map_err(invalid)?;
        let destination = stage.join("bin").join(
            if cfg!(windows) && matches!(manager.as_str(), "npm" | "pypi") {
                format!("{alias}.cmd")
            } else if cfg!(windows) {
                format!("{alias}.exe")
            } else {
                alias.clone()
            },
        );
        if manager == "pypi" && entries.contains_key(binary) {
            // Console-script launchers embed their original venv path; resolve the
            // registry-declared entry point through the relocated venv instead.
            let python = final_path.join(if cfg!(windows) {
                ".venv/Scripts/python.exe"
            } else {
                ".venv/bin/python"
            });
            let entry = format!("'{binary}'");
            let script = format!(
                "import importlib.metadata, sys\n\
                 entry = next((entry for distribution in importlib.metadata.distributions() \
                 for entry in distribution.entry_points \
                 if entry.group == 'console_scripts' and entry.name == {entry}), None)\n\
                 sys.exit(entry.load()() if entry else 127)\n"
            );
            launcher(&destination, &python, &["-c".into(), script])?;
        } else if matches!(manager.as_str(), "npm" | "pypi") {
            launcher(&destination, &final_path.join(relative), &[])?;
        } else if destination != source {
            std::fs::copy(&source, &destination).map_err(invalid)?;
            archive::executable(&destination, 0o755)?;
        }
    }
    Ok(())
}

async fn run(
    executable: &Path,
    args: &[String],
    directory: &Path,
    environment: &crate::process::Environment,
    overrides: &BTreeMap<String, std::ffi::OsString>,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let mut command = tokio::process::Command::new(executable);
    environment.apply(&mut command);
    command
        .args(args)
        .current_dir(directory)
        .env("CI", "1")
        .envs(overrides)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    let mut child = command.spawn().map_err(invalid)?;
    let mut stderr = child
        .stderr()
        .take()
        .ok_or_else(|| Fault::invalid("language installer stderr is unavailable"))?;
    let mut errors = tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut buffer = [0u8; 4096];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..count]);
            if tail.len() > 64 * 1024 {
                tail.drain(..tail.len() - 64 * 1024);
            }
        }
        tail
    });
    let wait = async {
        // SAFETY: the process-group wrapper exclusively owns this native child;
        // the root wait prevents descendants from holding cancellation open.
        unsafe { child.try_inner_child_mut() }
            .expect("language installer child is native")
            .wait()
            .await
    };
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(
            ErrorCode::Cancelled,
            "language installation was cancelled",
        )),
        result = tokio::time::timeout(Duration::from_secs(20 * 60), wait) => {
            result
                .map_err(|_| Fault::new(ErrorCode::Unavailable, "language installation timed out"))
                .and_then(|result| result.map_err(invalid))
        },
    };
    let _ = child.start_kill();
    // SAFETY: only this owner observes the root after terminating its group.
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        unsafe { child.try_inner_child_mut() }
            .expect("language installer child is native")
            .wait(),
    )
    .await;
    let tail = match tokio::time::timeout(Duration::from_secs(1), &mut errors).await {
        Ok(Ok(tail)) => tail,
        _ => {
            errors.abort();
            let _ = errors.await;
            Vec::new()
        }
    };
    let status = result?;
    if !status.success() {
        let message = String::from_utf8_lossy(&tail);
        return Err(Fault::new(
            ErrorCode::Unavailable,
            format!("language installer exited with {status}: {message}"),
        ));
    }
    Ok(())
}

fn launcher(path: &Path, program: &Path, args: &[String]) -> Result<(), Fault> {
    #[cfg(unix)]
    {
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let mut line = format!("#!/bin/sh\nexec {}", quote(&program.to_string_lossy()));
        for arg in args {
            line.push(' ');
            line.push_str(&quote(arg));
        }
        line.push_str(" \"$@\"\n");
        std::fs::write(path, line).map_err(invalid)?;
        archive::executable(path, 0o755)?;
    }
    #[cfg(windows)]
    {
        fn quote(value: &str) -> Result<String, Fault> {
            if value.contains(['\r', '\n', '"']) {
                return Err(Fault::invalid(
                    "language launcher argument cannot be represented",
                ));
            }
            Ok(format!("\"{}\"", value.replace('%', "%%")))
        }
        let mut line = format!("@echo off\r\ncall {}", quote(&program.to_string_lossy())?);
        for arg in args {
            line.push(' ');
            line.push_str(&quote(arg)?);
        }
        line.push_str(" %*\r\n");
        std::fs::write(path, line).map_err(invalid)?;
    }
    Ok(())
}
fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(error.to_string())
}

#[cfg(all(test, unix))]
mod tests;
