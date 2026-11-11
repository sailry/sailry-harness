# Projects and sessions

Choose a host before adding a project. Project paths refer to that host's files,
so a remote project does not need a matching folder on your Desktop machine.

## Add a project

Use **Add project** and choose **Local** for an existing directory or **Clone** for
a Git repository. Enter the project name and path; cloning also needs a repository
URL and destination. For a remote project, select the remote host in the form.

When Sailry asks you to trust a project, review its source first. Project
instructions and tools can run code across its worktrees. **Revoke trust** stops
project tasks while keeping history and drafts.

## Work in a session

Choose **Create session** from the project's menu. Select a model in the composer
and describe your task. Sessions belong to the selected host, project and worktree;
opening another session does not move its files or execution to another machine.

Use the session menu to rename, pin or archive a conversation. Archived sessions
remain in **Session records** and can be restored. **Delete session** permanently
removes that session and its subagent sessions; stop active work before deleting.

## Use a Git worktree

For an isolated branch checkout, use the project's **Manage worktrees → New worktree**
action. Choose a starting ref, new branch and destination path. The repository
needs an existing commit. Open the worktree before creating the session that
should use it; each worktree has its own files and terminals.

Worktree removal keeps the Git branch but deletes the checkout directory.
Save and commit changes first, close its terminals, and resolve any sessions
that still reference it, including archived sessions. The main worktree cannot be removed.

Removing a project from the project list keeps its files and session history.
Stop active project work first; this is different from deleting a worktree or session.
