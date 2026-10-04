//! An out-of-process installer for the Windows portable application bundle.
//!
//! The helper runs from a private copy outside both bundle trees. Only the shared
//! updater callback authenticates releases; this module owns the process handoff.

mod bundle;
#[cfg(target_os = "windows")]
mod process;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
#[cfg(target_os = "windows")]
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const HELPER_FLAG: &str = "--sailry-update-helper";
const PLAN_NAME: &str = "plan.json";
// The signed manifest is serialized as a byte array inside the opaque proof;
// a 1 MiB manifest can require about 4 MiB in the private JSON plan.
const MAX_PLAN_BYTES: u64 = 8 * 1024 * 1024;

/// The shared callback returns only an authenticated, extracted package.
pub(super) struct VerifiedBundle {
    pub root: PathBuf,
    pub executable: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u32,
    parent_pid: u32,
    parent_started: u64,
    archive: PathBuf,
    proof: Value,
    install_root: PathBuf,
    executable: PathBuf,
    restart_args: Vec<OsString>,
    receipt: PathBuf,
}

#[derive(Serialize)]
struct Receipt<'a> {
    version: u32,
    result: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    installed: bool,
    uncertain: bool,
}

#[cfg(target_os = "windows")]
pub(super) struct PreparedHelper {
    workspace: tempfile::TempDir,
    executable: PathBuf,
    plan: PathBuf,
}

#[cfg(target_os = "windows")]
pub(super) fn system_version() -> Result<String, String> {
    process::system_version()
}

/// Preparation does not stop the Node or alter the installed bundle. The caller
/// must save recovery state and finish the local Node shutdown before exiting.
#[cfg(target_os = "windows")]
pub(super) fn prepare(
    archive: &Path,
    proof: Value,
    install_root: &Path,
    executable_relative: &Path,
    restart_args: &[OsString],
    receipt: &Path,
) -> Result<PreparedHelper, String> {
    relative_executable(executable_relative)?;
    let install_root = plain_directory(install_root)?;
    let installed_executable = plain_file(&install_root.join(executable_relative))?;
    let current = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| format!("cannot resolve the running executable: {error}"))?;
    if installed_executable != current {
        return Err("installation does not contain the running executable".into());
    }
    let receipt = receipt_path(receipt)?;
    if receipt.starts_with(&install_root) {
        return Err("update recovery state cannot be inside the application bundle".into());
    }
    let archive = plain_file(archive)?;
    let parent = install_root
        .parent()
        .filter(|parent| parent.parent().is_some())
        .ok_or("application bundle has no installation parent")?;
    let workspace = tempfile::Builder::new()
        .prefix(".sailry-update-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create the update workspace: {error}"))?;
    process::protect(workspace.path())?;
    let helper_root = workspace.path().join("helper");
    bundle::copy_tree(&install_root, &helper_root)?;
    let archives = workspace.path().join("archive");
    fs::create_dir(&archives)
        .map_err(|error| format!("cannot create the helper archive directory: {error}"))?;
    let copied_archive = archives.join(archive.file_name().ok_or("archive has no file name")?);
    fs::copy(&archive, &copied_archive)
        .map_err(|error| format!("cannot copy the staged archive: {error}"))?;
    process::protect(&copied_archive)?;
    let plan = Plan {
        version: 1,
        parent_pid: std::process::id(),
        parent_started: process::current_started()?,
        archive: copied_archive,
        proof,
        install_root,
        executable: executable_relative.to_path_buf(),
        restart_args: restart_args.to_vec(),
        receipt,
    };
    let plan_path = workspace.path().join(PLAN_NAME);
    write_json(&plan_path, &plan)?;
    Ok(PreparedHelper {
        executable: helper_root.join(executable_relative),
        workspace,
        plan: plan_path,
    })
}

#[cfg(target_os = "windows")]
impl PreparedHelper {
    /// The ready acknowledgment proves the child has authenticated/extracted the
    /// package and opened the original process handle. It is not installation.
    pub(super) fn launch(self) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

        let mut child = Command::new(&self.executable)
            .arg(HELPER_FLAG)
            .arg(&self.plan)
            .current_dir(self.workspace.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("cannot start the update helper: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(120);
        let ready = self.workspace.path().join("ready");
        let failed = self.workspace.path().join("failure.json");
        let readiness = loop {
            if ready.is_file() {
                break write_new(&self.workspace.path().join("handoff"), b"confirmed");
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    let message = read_failure(&failed).unwrap_or_else(|| {
                        format!("update helper exited before handoff: {status}")
                    });
                    break Err(message);
                }
                Ok(None) => {}
                Err(error) => break Err(format!("cannot inspect the update helper: {error}")),
            }
            if Instant::now() >= deadline {
                break Err("update helper did not become ready".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        if let Err(error) = readiness {
            // No handoff was authorized, so even a child that cannot be stopped
            // must not install after a later ordinary application exit.
            let _kill = child.kill();
            if let Err(wait_error) = child.wait() {
                let retained = self.workspace.keep();
                return Err(format!(
                    "{error}; cannot finish helper cleanup: {wait_error}; workspace retained at {}",
                    retained.display()
                ));
            }
            let retained = self.workspace.keep();
            return Err(format!(
                "{error}; update workspace retained at {}",
                retained.display()
            ));
        }
        // The child still uses its copy, including loader DLLs. Retain the
        // workspace and old bundle for recovery; never clean them here.
        let _workspace = self.workspace.keep();
        Ok(())
    }
}

/// Dispatch before starting a Node, parsing ordinary launch options, or GPUI.
#[cfg(target_os = "windows")]
pub(super) fn run_if_requested(
    args: &[OsString],
    verify: impl Fn(&Path, &Value, &Path) -> Result<VerifiedBundle, String>,
) -> Option<Result<(), String>> {
    let path = match helper_request(args)? {
        Ok(path) => path,
        Err(error) => return Some(Err(error)),
    };
    Some(run(&path, verify))
}

#[cfg(target_os = "windows")]
fn run(
    path: &Path,
    verify: impl Fn(&Path, &Value, &Path) -> Result<VerifiedBundle, String>,
) -> Result<(), String> {
    let (workspace, plan) = read_plan(path)?;
    let mut receipt_written = false;
    let mut installed = false;
    let mut installation_started = false;
    let result: Result<(), String> = (|| {
        let current = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|error| format!("cannot resolve the helper executable: {error}"))?;
        if current != plain_file(&workspace.join("helper").join(&plan.executable))? {
            return Err("update helper must run from its private bundle copy".into());
        }
        let parent = process::Parent::open(plan.parent_pid, plan.parent_started)?;
        let staging = workspace.join("staging");
        fs::create_dir(&staging)
            .map_err(|error| format!("cannot create helper staging: {error}"))?;
        let verified = verify(&plan.archive, &plan.proof, &staging)?;
        validate_bundle(&workspace, &plan, &verified)?;
        write_new(&workspace.join("ready"), b"ready")?;
        parent.wait(Duration::from_secs(180))?;
        let handoff = plain_file(&workspace.join("handoff"))?;
        if fs::read(handoff).map_err(|error| format!("cannot read update handoff: {error}"))?
            != b"confirmed"
        {
            return Err(
                "update handoff was not authorized; the installed bundle was not changed".into(),
            );
        }
        installation_started = true;
        install(&workspace, &plan, &verified)?;
        installed = true;
        write_receipt(&plan.receipt, "success", None)?;
        receipt_written = true;
        restart(&plan)?;
        Ok(())
    })();
    if let Err(error) = &result {
        let message = format!(
            "{error}; update workspace retained at {}",
            workspace.display()
        );
        let recovery = if receipt_written {
            replace_receipt(&plan.receipt, "failure", &message)
        } else {
            write_json(
                &plan.receipt,
                &Receipt {
                    version: 1,
                    result: "failure",
                    message: Some(&message),
                    installed,
                    uncertain: installation_started && !installed,
                },
            )
        };
        let _failure = write_json(
            &workspace.join("failure.json"),
            &Receipt {
                version: 1,
                result: "failure",
                message: Some(&message),
                installed,
                uncertain: installation_started && !installed,
            },
        );
        if let Err(receipt_error) = recovery {
            return Err(format!(
                "{message}; cannot save recovery receipt: {receipt_error}"
            ));
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn restart(plan: &Plan) -> Result<(), String> {
    Command::new(plan.install_root.join(&plan.executable))
        .args(&plan.restart_args)
        .current_dir(&plan.install_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
        .map_err(|error| format!("the update was installed but Sailry could not restart: {error}"))
}

fn helper_request(args: &[OsString]) -> Option<Result<PathBuf, String>> {
    if args.first().is_none_or(|arg| arg != HELPER_FLAG) {
        return None;
    }
    Some(match args {
        [_, path] => Ok(PathBuf::from(path)),
        _ => Err("update helper requires exactly one plan path".into()),
    })
}

fn read_plan(path: &Path) -> Result<(PathBuf, Plan), String> {
    let path = plain_file(path)?;
    if path.file_name().is_none_or(|name| name != PLAN_NAME) {
        return Err("update plan has an invalid file name".into());
    }
    let workspace = plain_directory(path.parent().ok_or("update plan has no parent")?)?;
    if workspace
        .file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| !name.starts_with(".sailry-update-"))
    {
        return Err("update plan is outside an updater workspace".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|error| format!("cannot open the update plan: {error}"))?
        .take(MAX_PLAN_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read the update plan: {error}"))?;
    if bytes.len() as u64 > MAX_PLAN_BYTES {
        return Err("update plan exceeds the size limit".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot decode the update plan: {error}"))?;
    validate_plan(&workspace, &plan)?;
    Ok((workspace, plan))
}

fn validate_plan(workspace: &Path, plan: &Plan) -> Result<(), String> {
    if plan.version != 1 || plan.parent_pid == 0 || plan.parent_started == 0 {
        return Err("update plan has an unsupported definition".into());
    }
    relative_executable(&plan.executable)?;
    let installed = plain_directory(&plan.install_root)?;
    if installed != plan.install_root || workspace.parent() != installed.parent() {
        return Err("update workspace must be beside the installed bundle".into());
    }
    if workspace.starts_with(&installed) || installed.starts_with(workspace) {
        return Err("update workspace overlaps the installed bundle".into());
    }
    let archive = plain_file(&plan.archive)?;
    if archive != plan.archive || archive.parent() != Some(workspace.join("archive").as_path()) {
        return Err("update archive is outside the private workspace".into());
    }
    let receipt = receipt_path(&plan.receipt)?;
    if receipt != plan.receipt || receipt.starts_with(&installed) || receipt.starts_with(workspace)
    {
        return Err("update receipt has an invalid location".into());
    }
    if plan.restart_args.iter().any(|arg| arg == HELPER_FLAG) {
        return Err("update helper arguments cannot be used for relaunch".into());
    }
    Ok(())
}

fn validate_bundle(workspace: &Path, plan: &Plan, verified: &VerifiedBundle) -> Result<(), String> {
    relative_executable(&verified.executable)?;
    let root = plain_directory(&verified.root)?;
    let staging = plain_directory(&workspace.join("staging"))?;
    if root == staging || !root.starts_with(&staging) || verified.executable != plan.executable {
        return Err("verified package has an invalid bundle location".into());
    }
    let executable = plain_file(&root.join(&verified.executable))?;
    if !executable.starts_with(&root) {
        return Err("verified executable escapes its bundle".into());
    }
    bundle::validate_tree(&root)?;
    Ok(())
}

fn install(workspace: &Path, plan: &Plan, verified: &VerifiedBundle) -> Result<(), String> {
    validate_bundle(workspace, plan, verified)?;
    let backup = workspace.join("previous");
    bundle::replace(&verified.root, &plan.install_root, &backup)
}

fn relative_executable(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("bundle executable must be a confined relative path".into());
    }
    Ok(())
}

fn plain_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "cannot inspect update directory {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_dir() || bundle::is_link(&metadata) {
        return Err(format!(
            "update path is not an ordinary directory: {}",
            path.display()
        ));
    }
    fs::canonicalize(path).map_err(|error| {
        format!(
            "cannot resolve update directory {}: {error}",
            path.display()
        )
    })
}

fn plain_file(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect update file {}: {error}", path.display()))?;
    if !metadata.is_file() || bundle::is_link(&metadata) {
        return Err(format!(
            "update path is not an ordinary file: {}",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve update file {}: {error}", path.display()))
}

fn receipt_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("update receipt must have an absolute file path".into());
    }
    let parent = plain_directory(path.parent().ok_or("update receipt has no parent")?)?;
    let path = parent.join(path.file_name().ok_or("update receipt has no name")?);
    match fs::symlink_metadata(&path) {
        Ok(_) => return Err("update receipt already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect the update receipt: {error}")),
    }
    Ok(path)
}

fn write_receipt(path: &Path, result: &str, message: Option<&str>) -> Result<(), String> {
    write_json(
        path,
        &Receipt {
            version: 1,
            result,
            message,
            installed: result == "success",
            uncertain: false,
        },
    )
}

#[cfg(target_os = "windows")]
fn replace_receipt(path: &Path, result: &str, message: &str) -> Result<(), String> {
    // This path was created by this helper after installation. Replace only its
    // own success receipt when process creation fails, not arbitrary state.
    let bytes = serde_json::to_vec(&Receipt {
        version: 1,
        result,
        message: Some(message),
        installed: true,
        uncertain: false,
    })
    .map_err(|error| format!("cannot encode update recovery: {error}"))?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("receipt has no parent")?)
            .map_err(|error| format!("cannot stage the recovery receipt: {error}"))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| format!("cannot save the recovery receipt: {error}"))?;
    temporary
        .persist(path)
        .map(|_file| ())
        .map_err(|error| format!("cannot replace the recovery receipt: {error}"))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("cannot encode updater state: {error}"))?;
    if bytes.len() as u64 > MAX_PLAN_BYTES {
        return Err("updater state exceeds the size limit".into());
    }
    write_new(path, &bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("cannot create updater state {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot save updater state {}: {error}", path.display()))
}

#[cfg(target_os = "windows")]
fn read_failure(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_PLAN_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let receipt: Value = serde_json::from_slice(&bytes).ok()?;
    receipt.get("message")?.as_str().map(str::to_owned)
}
