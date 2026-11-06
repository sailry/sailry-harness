use super::super::{head, open, path};
use crate::Node;
use git2::{Config, Repository, RepositoryOpenFlags};
use sailry_client::Client;
use sailry_protocol::{Command, ErrorCode, Fault, Output};
use std::{ffi::OsStr, path::Path};

#[tokio::test]
#[ignore = "temporary read-only Windows probe; requires SAILRY_GIT_ROOT_PROBE naming the synthetic acceptance repository"]
async fn registered_root() {
    let input = std::env::var_os("SAILRY_GIT_ROOT_PROBE")
        .expect("SAILRY_GIT_ROOT_PROBE must explicitly select the synthetic acceptance repository");
    let input = Path::new(&input);
    let authorized = Path::new(r"F:\sailry-harness\.runtime\windows\gui-project")
        .canonicalize()
        .expect("synthetic acceptance repository must exist");
    assert_eq!(
        input.canonicalize().unwrap(),
        authorized,
        "the probe is restricted to the synthetic acceptance repository"
    );
    boundaries("input", input);
    let temporary = tempfile::tempdir().unwrap();
    let profile = temporary.path().join("node");
    eprintln!("probe.profile_prefix={:?}", profile.components().next());
    let node = Node::start_empty(profile).await.unwrap();
    let client = Client::new(node.local());
    let result = registered(&client, input).await;
    eprintln!("probe.service_result={result:#?}");
    node.shutdown().await.unwrap();
    result.unwrap();
}

async fn registered(client: &Client, input: &Path) -> Result<(), Fault> {
    let registration = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Git root probe".into(),
            path: input.to_str().unwrap().into(),
        }))
        .await;
    eprintln!("probe.RegisterProject={registration:#?}");
    let Output::Project(project) = registration? else {
        return Err(Fault::new(ErrorCode::Internal, "project expected"));
    };
    let Output::Snapshot(snapshot) = client.execute(client.prepare(Command::Snapshot)).await?
    else {
        return Err(Fault::new(ErrorCode::Internal, "snapshot expected"));
    };
    let worktree = snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.project == Some(project.id) && worktree.main)
        .ok_or_else(|| Fault::new(ErrorCode::Internal, "main worktree expected"))?;
    boundaries("registered", Path::new(&worktree.path));
    let response = client
        .execute(client.prepare(Command::ReadGitHead {
            worktree: worktree.id,
        }))
        .await;
    eprintln!("probe.ReadGitHead={response:#?}");
    match response? {
        Output::GitHead(_) => Ok(()),
        _ => Err(Fault::new(ErrorCode::Internal, "Git HEAD expected")),
    }
}

fn boundaries(stage: &str, root: &Path) {
    eprintln!(
        "probe.{stage}.path={root:?} prefix={:?} absolute={}",
        root.components().next(),
        root.is_absolute()
    );
    eprintln!("probe.{stage}.canonical={:?}", root.canonicalize());
    match path::root(root) {
        Ok(retained) => {
            eprintln!(
                "probe.{stage}.identity={:?}",
                path::identity::directory(&retained)
            );
            match retained.symlink_metadata(".git") {
                Ok(metadata) => eprintln!(
                    "probe.{stage}.metadata file={} directory={} symlink={}",
                    metadata.is_file(),
                    metadata.is_dir(),
                    metadata.is_symlink()
                ),
                Err(error) => eprintln!("probe.{stage}.metadata={error:?}"),
            }
        }
        Err(error) => eprintln!("probe.{stage}.capability_root={error:#?}"),
    }
    match Repository::open_ext(
        root,
        RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&OsStr>(),
    ) {
        Ok(repository) => {
            if let Some(workdir) = repository.workdir() {
                eprintln!("probe.{stage}.workdir={workdir:?}");
                match workdir.canonicalize() {
                    Ok(canonical) => eprintln!(
                        "probe.{stage}.workdir_canonical={canonical:?} exact_root={}",
                        canonical == root
                    ),
                    Err(error) => eprintln!("probe.{stage}.workdir_canonical={error:?}"),
                }
            } else {
                eprintln!("probe.{stage}.workdir=None");
            }
            if let Err(error) = repository.set_config(&Config::new().unwrap()) {
                raw_fault(stage, "set_config", error);
            }
            match repository.head() {
                Ok(reference) => eprintln!(
                    "probe.{stage}.head branch={} name={:?} target={:?}",
                    reference.is_branch(),
                    reference.name(),
                    reference.target()
                ),
                Err(error) => raw_fault(stage, "head", error),
            }
            match repository.find_reference("HEAD") {
                Ok(reference) => eprintln!(
                    "probe.{stage}.symbolic_target={:?}",
                    reference.symbolic_target()
                ),
                Err(error) => raw_fault(stage, "find_reference", error),
            }
            eprintln!("probe.{stage}.projection={:#?}", head::project(&repository));
        }
        Err(error) => raw_fault(stage, "open_ext", error),
    }
    match open(root) {
        Ok(_) => eprintln!("probe.{stage}.guarded_open=Ok"),
        Err(error) => eprintln!("probe.{stage}.guarded_open={error:#?}"),
    }
}

fn raw_fault(stage: &str, operation: &str, error: git2::Error) {
    eprintln!(
        "probe.{stage}.{operation} code={:?} class={:?} message={:?}",
        error.code(),
        error.class(),
        error.message()
    );
}
