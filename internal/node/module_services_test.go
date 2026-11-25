package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func requireValidModuleRequest(t *testing.T, request protocol.Request) {
	t.Helper()
	encoded, err := json.Marshal(request)
	if err != nil {
		t.Fatalf("encode %s fixture: %v", request.Command.Kind, err)
	}
	var decoded protocol.Request
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatalf("decode %s fixture: %v", request.Command.Kind, err)
	}
}

func requireModuleUnavailable(t *testing.T, err error) {
	t.Helper()
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeNotConfigured {
		t.Fatalf("expected unavailable module, got %v", err)
	}
}

func TestUnassembledConnectionCommandsHaveNoEffects(t *testing.T) {
	var calls atomic.Int32
	node := serviceTestNode(t, moduleapi.Assembly{
		Databases: func(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error) {
			calls.Add(1)
			return protocol.DatabaseOutcome{}, nil
		},
		SSH: func(moduleapi.SSHServiceOptions) moduleapi.SSHService { calls.Add(1); return &serviceTestSSH{} },
		Terminal: func(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service {
			calls.Add(1)
			return nil
		},
	})
	if node.workspaceRuntime != nil || node.terminals != nil || node.ssh != nil || node.executeDatabase != nil || calls.Load() != 0 {
		t.Fatal("unassembled service was initialized")
	}
	path := filepath.Join(t.TempDir(), "legacy.sqlite")
	database := protocol.DefaultCommandSaveDatabasePayload()
	database.Profile.ID, database.Profile.Name = protocol.NewDatabaseID(), "Legacy database"
	database.Profile.Connection = protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: path}}
	ssh := protocol.DefaultCommandSaveSshPayload()
	ssh.Profile.ID, ssh.Profile.Name, ssh.Profile.Host, ssh.Profile.Port, ssh.Profile.Username = protocol.NewSshID(), "Legacy SSH", "127.0.0.1", 22, "fixture"
	ssh.Profile.Authentication = protocol.SshAuthenticationPassword
	ssh.Credential = &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("legacy-private")}}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, _, err := store.ExecuteDatabaseCommand(t.Context(), db, protocol.Command{Kind: "save_database", Data: &database})
		if err != nil {
			return err
		}
		_, _, err = store.ExecuteSshCommand(t.Context(), db, protocol.Command{Kind: "save_ssh", Data: &ssh})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	state := func() string {
		t.Helper()
		var encoded []byte
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			databases, err := store.ListDatabases(t.Context(), db)
			if err != nil {
				return err
			}
			profiles, err := store.ListSsh(t.Context(), db)
			if err != nil {
				return err
			}
			var requests int
			if err = db.QueryRowContext(t.Context(), "SELECT COUNT(*) FROM requests").Scan(&requests); err != nil {
				return err
			}
			encoded, err = json.Marshal([]any{databases, profiles, requests})
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return string(encoded)
	}
	before := state()
	cancelled := protocol.NewRequestID()
	terminalID, streamID := protocol.NewTerminalID(), protocol.NewStreamID()
	viewport := protocol.TerminalViewport{Columns: 80, Rows: 24, PixelWidth: 640, PixelHeight: 480}
	appearance := protocol.DefaultTerminalAppearance()
	appearance.ColorScheme = protocol.TerminalColorSchemeDark
	commands := []protocol.Command{
		{Kind: "list_databases"},
		{Kind: "save_database", Data: database},
		{Kind: "remove_database", Data: protocol.CommandRemoveDatabasePayload{Profile: database.Profile.ID, ExpectedRevision: 1}},
		{Kind: "test_database", Data: protocol.CommandTestDatabasePayload{Profile: database.Profile}},
		{Kind: "check_database", Data: protocol.CommandCheckDatabasePayload{Profile: database.Profile.ID, ExpectedRevision: 1}},
		{Kind: "browse_database", Data: protocol.CommandBrowseDatabasePayload{Profile: database.Profile.ID, ExpectedRevision: 1}},
		{Kind: "query_database", Data: protocol.CommandQueryDatabasePayload{Profile: database.Profile.ID, ExpectedRevision: 1, Sql: "CREATE TABLE side_effect(value)", RowLimit: 1, TimeoutMs: 1000}},
		{Kind: "cancel_database", Data: protocol.CommandCancelDatabasePayload{Request: cancelled}},
		{Kind: "list_ssh"},
		{Kind: "save_ssh", Data: ssh},
		{Kind: "remove_ssh", Data: protocol.CommandRemoveSshPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1}},
		{Kind: "trust_ssh", Data: protocol.CommandTrustSshPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Key: protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}}},
		{Kind: "check_ssh", Data: protocol.CommandCheckSshPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1}},
		{Kind: "run_ssh", Data: protocol.CommandRunSshPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Command: "touch side_effect", TimeoutMs: 1000}},
		{Kind: "browse_ssh_directory", Data: protocol.CommandBrowseSshDirectoryPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Path: "/tmp"}},
		{Kind: "download_ssh_file", Data: protocol.CommandDownloadSshFilePayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Path: "/tmp/source.txt"}},
		{Kind: "modify_ssh_file", Data: protocol.CommandModifySshFilePayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Path: "/tmp/side_effect.txt", Action: protocol.SshFileAction{Kind: "create", Data: protocol.SshFileActionCreatePayload{}}}},
		{Kind: "transfer_ssh", Data: protocol.CommandTransferSshPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, TimeoutMs: 1000, Transfer: protocol.SshTransfer{Worktree: protocol.NewWorktreeID(), Path: "source.txt", RemotePath: "/tmp/side_effect.txt", Direction: protocol.SshDirectionUpload}}},
		{Kind: "stage_ssh_upload", Data: protocol.SshUploadSpec{Profile: ssh.Profile.ID, ExpectedRevision: 1, Path: "/tmp/side_effect.txt", Size: 1, Revision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
		{Kind: "finish_ssh_upload", Data: protocol.CommandFinishSshUploadPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Path: "/tmp/side_effect.txt", Stream: streamID}},
		{Kind: "install_host", Data: protocol.CommandInstallHostPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1}},
		{Kind: "update_ssh_host", Data: protocol.CommandUpdateSshHostPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Node: node.ID()}},
		{Kind: "read_host_install", Data: protocol.CommandReadHostInstallPayload{Request: cancelled}},
		{Kind: "cancel_ssh", Data: protocol.CommandCancelSshPayload{Request: cancelled}},
		{Kind: "open_ssh_terminal", Data: protocol.CommandOpenSshTerminalPayload{Profile: ssh.Profile.ID, ExpectedRevision: 1, Launch: protocol.SshTerminalLaunch{Viewport: viewport, Appearance: appearance}}},
		{Kind: "close_ssh_terminal", Data: protocol.CommandCloseSshTerminalPayload{Terminal: terminalID}},
		{Kind: "list_ssh_terminals", Data: protocol.CommandListSshTerminalsPayload{Profile: ssh.Profile.ID}},
	}
	for _, command := range commands {
		t.Run(command.Kind, func(t *testing.T) {
			request := protocol.NewRequest(node.ID(), command)
			requireValidModuleRequest(t, request)
			admitted, err := node.Dispatch(t.Context(), node.ID(), request)
			requireModuleUnavailable(t, err)
			if admitted.Completion != nil {
				t.Fatal("unavailable command was admitted")
			}
			if _, err := node.runHost(t.Context(), node.ID(), request); databaseCommand(command.Kind) || sshResourceCommand(command.Kind) {
				requireModuleUnavailable(t, err)
			}
		})
	}
	if after := state(); after != before {
		t.Fatalf("unassembled commands changed stored profiles or admission: before=%s after=%s", before, after)
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("database command created an external file: %v", err)
	}
	if calls.Load() != 0 {
		t.Fatal("unassembled command reached an injected service")
	}
}

func TestPluginRuntimeRequiresCodeIndexService(t *testing.T) {
	node := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{serviceTestModule("code-index"), serviceTestModule("storage")}})
	if modules := node.pluginRuntime().Modules; len(modules) != 1 || modules[0] != "storage" {
		t.Fatalf("modules advertised without a code index service: %v", modules)
	}
}

func TestUnassembledDevelopmentCommandsAreRejected(t *testing.T) {
	node := serviceTestNode(t, moduleapi.Assembly{})
	directory := t.TempDir()
	project := commandFixture(t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Ordinary directory", Path: directory}}).Data.(protocol.Project)
	var worktree protocol.WorktreeID
	for _, tree := range snapshotFixture(t, node).Worktrees {
		if tree.Project != nil && *tree.Project == project.ID && tree.Main {
			worktree = tree.ID
		}
	}
	if worktree == (protocol.WorktreeID{}) {
		t.Fatal("ordinary project has no main worktree")
	}
	terminalID := protocol.NewTerminalID()
	worktreePath := filepath.Join(t.TempDir(), "side_effect")
	head := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	index := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	appearance := protocol.DefaultTerminalAppearance()
	appearance.ColorScheme = protocol.TerminalColorSchemeDark
	commands := []protocol.Command{
		{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: worktree}},
		{Kind: "run_git_action", Data: protocol.CommandRunGitActionPayload{Worktree: worktree, Action: protocol.GitAction{Kind: "initialize"}, ExpectedIndex: index}},
		{Kind: "create_git_commit", Data: protocol.CommandCreateGitCommitPayload{Worktree: worktree, Message: "Unavailable commit", Options: protocol.DefaultGitCommitOptions(), ExpectedIndex: index}},
		{Kind: "create_worktree", Data: protocol.CommandCreateWorktreePayload{Project: project.ID, Path: worktreePath, Branch: "side-effect", Commit: head}},
		{Kind: "create_managed_worktree", Data: protocol.CommandCreateManagedWorktreePayload{Project: project.ID, Source: worktree, Branch: "side-effect", ExpectedHead: head, ExpectedIndex: index}},
		{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project.ID, Path: worktreePath}},
		{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: worktree, ExpectedHead: head, ExpectedBranch: "main"}},
		{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: worktree}},
		{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "Unavailable clone", Path: worktreePath, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "clone", Data: &protocol.ProjectsSourceClonePayload{Url: directory}}}},
		{Kind: "read_terminal_settings"},
		{Kind: "save_terminal_settings", Data: protocol.DefaultTerminalSettings()},
		{Kind: "list_shells"},
		{Kind: "create_terminal", Data: protocol.TerminalLaunch{Worktree: worktree, Access: protocol.ProcessAccessWorkspace, Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24, PixelWidth: 640, PixelHeight: 480}, Appearance: appearance}},
		{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{Terminal: terminalID, Revision: 1, Bytes: protocol.Bytes("echo side_effect\n")}},
		{Kind: "claim_terminal", Data: protocol.CommandClaimTerminalPayload{Terminal: terminalID, ExpectedRevision: 1}},
	}
	for _, command := range commands {
		t.Run(command.Kind, func(t *testing.T) {
			request := protocol.NewRequest(node.ID(), command)
			requireValidModuleRequest(t, request)
			admitted, err := node.Dispatch(t.Context(), node.ID(), request)
			requireModuleUnavailable(t, err)
			if admitted.Completion != nil {
				t.Fatal("unavailable development command was admitted")
			}
		})
	}
	for _, path := range []string{worktreePath, filepath.Join(directory, ".git")} {
		if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("unassembled command created %s: %v", path, err)
		}
	}
	destination := filepath.Join(t.TempDir(), "clone")
	draft := protocol.DefaultProjectsDraft()
	draft.Name, draft.Path = "Unavailable clone", destination
	draft.Source = protocol.ProjectsSource{Kind: "clone", Data: &protocol.ProjectsSourceClonePayload{Url: "https://unavailable.invalid/repository"}}
	_, err := node.prepare(t.Context(), protocol.Command{Kind: "create_project", Data: &draft})
	requireModuleUnavailable(t, err)
	if _, err := os.Stat(destination); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("unassembled clone created a directory: %v", err)
	}
	if _, err := node.subscribeWorkspace(t.Context(), protocol.Topic{Kind: "repository"}); err == nil {
		t.Fatal("repository subscription started without Git")
	}
	if _, err := node.subscribeTerminal(protocol.Topic{}); err == nil {
		t.Fatal("terminal subscription started without terminal")
	}
	node.removeUnusedWorktree(protocol.Worktree{})
}

func TestUnassembledRuntimeKeepsGenericCommands(t *testing.T) {
	held := &connectionHeldModel{started: make(chan struct{}), release: make(chan struct{})}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	var executed atomic.Int32
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment, MCP: fixtureMCPOptions(), Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return held, nil }, Process: process.Options{
		Run: func(_ context.Context, launch process.Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
			executed.Add(1)
			emit(protocol.ProcessOutputOutputStreamStdout, []byte("generic command\n"))
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		},
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	t.Cleanup(func() { close(held.release) })
	directory := t.TempDir()
	project := commandFixture(t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Ordinary directory", Path: directory}}).Data.(protocol.Project)
	config := sessionConfigFixture(t, node)
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	commandFixture(t, node, protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "ordinary.txt", Text: "ordinary workspace"}})
	commandFixture(t, node, protocol.Command{Kind: "list_directory", Data: protocol.CommandListDirectoryPayload{Worktree: session.Worktree, Path: "."}})
	turn := commandFixture(t, node, protocol.Command{Kind: "submit_task", Data: protocol.CommandSubmitTaskPayload{Session: session.ID, Message: protocol.ConversationInput{Text: "Hold the generic command fixture"}}}).Data.(protocol.QueuedTurn)
	select {
	case <-held.started:
	case <-time.After(10 * time.Second):
		t.Fatal("generic task did not reach the scripted model")
	}
	launch := protocol.DefaultCommandRunCommandPayload()
	launch.Turn, launch.Command, launch.TimeoutMs = turn.ID, "fixture", 1000
	commandFixture(t, node, protocol.Command{Kind: "run_command", Data: launch})
	if executed.Load() != 1 || node.workspaceRuntime != nil || node.terminals != nil {
		t.Fatal("generic command requires a development service")
	}
	if node.processes == nil {
		t.Fatal("generic process service was omitted")
	}
	root, err := node.workspaceRoot(t.Context(), session.Worktree, false)
	if err != nil {
		t.Fatal(err)
	}
	for range 3 {
		subscription, err := node.subscribeWorkspace(t.Context(), protocol.Topic{Kind: "files", Data: session.Worktree})
		if err != nil {
			t.Fatal(err)
		}
		if update, err := subscription.Next(t.Context()); err != nil || update.Kind != "files_changed" {
			t.Fatalf("ordinary file subscription: %s %v", update.Kind, err)
		}
		if err := subscription.Close(); err != nil {
			t.Fatal(err)
		}
		node.watches.mu.Lock()
		retained := node.watches.roots[root] != nil
		node.watches.mu.Unlock()
		if retained {
			t.Fatal("ordinary file watch survived its subscription")
		}
	}
	if _, err := os.Stat(filepath.Join(root, ".sailry")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("ordinary directory acquired Git-managed data: %v", err)
	}
}

type serviceTestDatabaseModule struct {
	serviceTestModule
	database moduleapi.DatabaseExecutor
}

func (module serviceTestDatabaseModule) ExecuteDatabase(ctx context.Context, operation store.DatabaseOperation) (protocol.DatabaseOutcome, error) {
	return module.database(ctx, operation)
}

type serviceTestSSHModule struct {
	serviceTestModule
	ssh moduleapi.SSHFactory
}

func (module serviceTestSSHModule) NewSSHService(options moduleapi.SSHServiceOptions) moduleapi.SSHService {
	return module.ssh(options)
}

type serviceTestTerminalModule struct {
	serviceTestModule
	terminal moduleapi.TerminalFactory
}

func (module serviceTestTerminalModule) NewTerminalService(ctx context.Context, id protocol.NodeID, changed func(context.Context, protocol.TerminalInfo)) *terminal.Service {
	return module.terminal(ctx, id, changed)
}

type serviceTestWorkspaceModule struct {
	serviceTestModule
	services moduleapi.WorkspaceServices
}

func (module serviceTestWorkspaceModule) WorkspaceServices() moduleapi.WorkspaceServices {
	return module.services
}

func TestAssembledServiceFactoriesAndLifecycle(t *testing.T) {
	for _, mode := range []string{"legacy assembly", "module services", "assembly overrides"} {
		t.Run(mode, func(t *testing.T) { testAssembledServiceFactories(t, mode) })
	}
}

func testAssembledServiceFactories(t *testing.T, mode string) {
	t.Helper()
	var createdSSH, createdTerminal, executed, ignored atomic.Int32
	ssh := &serviceTestSSH{execute: func(_ context.Context, _ protocol.NodeID, request protocol.Request) (protocol.Output, error) {
		input := request.Command.Data.(*protocol.CommandCancelSshPayload)
		return protocol.Output{Kind: "ssh_cancelled", Data: protocol.OutputSshCancelledPayload{Request: input.Request}}, nil
	}}
	executor := func(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error) {
		executed.Add(1)
		return protocol.DatabaseOutcome{Kind: "connected"}, nil
	}
	factory := func(options moduleapi.SSHServiceOptions) moduleapi.SSHService {
		createdSSH.Add(1)
		if options.Context == nil || options.ReadSSH == nil || options.WorkspaceRoot == nil || options.Transfers == nil || options.Terminals == nil || options.Installations == nil || options.Link == nil {
			t.Error("SSH service lost a required Node boundary")
		}
		return ssh
	}
	terminalFactory := func(ctx context.Context, id protocol.NodeID, changed func(context.Context, protocol.TerminalInfo)) *terminal.Service {
		createdTerminal.Add(1)
		return terminal.New(ctx, id, changed)
	}
	assembly := moduleapi.Assembly{
		Modules:   []moduleapi.Module{serviceTestModule("databases"), serviceTestModule("ssh"), serviceTestModule("terminal")},
		Databases: executor,
		SSH:       factory,
		Terminal:  terminalFactory,
	}
	wantModules := []string{"databases", "ssh", "terminal"}
	if mode != "legacy assembly" {
		wantModules = []string{"fixture-db", "fixture-ssh", "fixture-terminal"}
		assembly.Modules = append([]moduleapi.Module{
			serviceTestDatabaseModule{serviceTestModule: "fixture-db", database: executor},
			serviceTestSSHModule{serviceTestModule: "fixture-ssh", ssh: factory},
			serviceTestTerminalModule{serviceTestModule: "fixture-terminal", terminal: terminalFactory},
		}, assembly.Modules...)
	}
	switch mode {
	case "module services":
		assembly.Databases, assembly.SSH, assembly.Terminal = nil, nil, nil
	case "assembly overrides":
		assembly.Modules[0] = serviceTestDatabaseModule{serviceTestModule: "fixture-db", database: func(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error) {
			ignored.Add(1)
			return protocol.DatabaseOutcome{}, errors.New("database override was ignored")
		}}
		assembly.Modules[1] = serviceTestSSHModule{serviceTestModule: "fixture-ssh", ssh: func(moduleapi.SSHServiceOptions) moduleapi.SSHService {
			ignored.Add(1)
			return nil
		}}
		assembly.Modules[2] = serviceTestTerminalModule{serviceTestModule: "fixture-terminal", terminal: func(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service {
			ignored.Add(1)
			return nil
		}}
	}
	node := serviceTestNode(t, assembly)
	if createdSSH.Load() != 1 || createdTerminal.Load() != 1 || node.terminals == nil || node.ssh != ssh {
		t.Fatal("assembled services were not initialized")
	}
	if actual := node.pluginRuntime().Modules; !slices.Equal(actual, wantModules) {
		t.Fatalf("advertised modules = %v", actual)
	}
	commandFixture(t, node, protocol.Command{Kind: "read_terminal_settings"})
	commandFixture(t, node, protocol.Command{Kind: "cancel_ssh", Data: protocol.CommandCancelSshPayload{Request: protocol.NewRequestID()}})
	profile := protocol.DefaultCommandSaveDatabasePayload()
	profile.Profile.ID, profile.Profile.Name = protocol.NewDatabaseID(), "Injected database"
	profile.Profile.Connection = protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: filepath.Join(t.TempDir(), "injected.sqlite")}}
	saved := commandFixture(t, node, protocol.Command{Kind: "save_database", Data: profile}).Data.(protocol.DatabaseProfile)
	commandFixture(t, node, protocol.Command{Kind: "check_database", Data: protocol.CommandCheckDatabasePayload{Profile: saved.ID, ExpectedRevision: saved.Revision}})
	if executed.Load() != 1 {
		t.Fatal("database operation did not reach the assembled executor")
	}
	if ignored.Load() != 0 {
		t.Fatal("an overridden default was used")
	}
	if err := node.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	if ssh.closed.Load() != 1 {
		t.Fatalf("SSH service closed %d times", ssh.closed.Load())
	}
}

func TestModuleNamesDoNotRegisterServices(t *testing.T) {
	assembly := moduleapi.Assembly{
		Modules: []moduleapi.Module{
			serviceTestModule("databases"), serviceTestModule("ssh"), serviceTestModule("terminal"),
			serviceTestModule("git"), serviceTestModule("worktrees"),
		},
	}
	node := serviceTestNode(t, assembly)
	if len(node.pluginRuntime().Modules) != 0 || node.runningModuleServices() != 0 {
		t.Fatal("module IDs registered or advertised unavailable services")
	}
	worktree := protocol.NewWorktreeID()
	for _, command := range []protocol.Command{
		{Kind: "list_databases"}, {Kind: "list_ssh"}, {Kind: "read_terminal_settings"},
		{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: worktree}},
		{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: worktree}},
	} {
		accepted, err := node.Dispatch(t.Context(), node.ID(), protocol.NewRequest(node.ID(), command))
		requireModuleUnavailable(t, err)
		if accepted.Completion != nil {
			t.Fatalf("%s was admitted", command.Kind)
		}
	}
}

func TestLegacyFactoriesDoNotAddServicesToTypedModule(t *testing.T) {
	assembly := moduleapi.Assembly{
		Modules: []moduleapi.Module{
			serviceTestWorkspaceModule{serviceTestModule: "git"},
			serviceTestWorkspaceModule{serviceTestModule: "terminal", services: moduleapi.WorkspaceServices{Worktrees: true, Bind: fixtureWorktreeBindings}},
		},
		LegacyWorkspaceModules: true,
		Terminal: func(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service {
			t.Fatal("terminal override ran for a typed worktree provider")
			return nil
		},
	}
	node := serviceTestNode(t, assembly)
	if node.runningModuleServices() != moduleapi.ServiceWorktrees || len(node.pluginRuntime().Modules) != 0 {
		t.Fatal("a typed module inherited a service or a requirement from its ID")
	}
	for _, command := range []protocol.Command{
		{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: protocol.NewWorktreeID()}},
		{Kind: "read_terminal_settings"},
	} {
		accepted, err := node.Dispatch(t.Context(), node.ID(), protocol.NewRequest(node.ID(), command))
		requireModuleUnavailable(t, err)
		if accepted.Completion != nil {
			t.Fatalf("%s inherited a handler", command.Kind)
		}
	}
}

func TestTerminalProviderWithoutRunningService(t *testing.T) {
	assembly := moduleapi.Assembly{
		Modules: []moduleapi.Module{serviceTestTerminalModule{serviceTestModule: "fixture-terminal", terminal: terminal.New}},
		Terminal: func(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service {
			return nil
		},
	}
	node := serviceTestNode(t, assembly)
	if len(node.pluginRuntime().Modules) != 0 {
		t.Fatal("terminal provider without a running service was advertised")
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_terminal_settings"})
	accepted, err := node.Dispatch(t.Context(), node.ID(), request)
	requireModuleUnavailable(t, err)
	if accepted.Completion != nil {
		t.Fatal("terminal settings command was admitted")
	}
	_, err = node.subscribeTerminal(protocol.Topic{Kind: "terminal", Data: protocol.NewTerminalID()})
	requireModuleUnavailable(t, err)
}

func TestWorkspaceCapabilitiesAreIndependentOfModuleIDs(t *testing.T) {
	for _, workspace := range []struct {
		id       string
		services moduleapi.WorkspaceServices
	}{
		{"repo-provider", moduleapi.WorkspaceServices{Git: true, Bind: fixtureGitBindings}},
		{"checkout-provider", moduleapi.WorkspaceServices{Worktrees: true, Bind: fixtureWorktreeBindings}},
	} {
		t.Run(workspace.id, func(t *testing.T) {
			node := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{
				serviceTestModule("git"), serviceTestModule("worktrees"),
				serviceTestWorkspaceModule{serviceTestModule: serviceTestModule(workspace.id), services: workspace.services},
			}})
			if runtime := node.pluginRuntime().Modules; !slices.Equal(runtime, []string{workspace.id}) {
				t.Fatalf("advertised modules = %v", runtime)
			}
			project := commandFixture(t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Workspace capability", Path: t.TempDir()}}).Data.(protocol.Project)
			var worktree protocol.WorktreeID
			for _, tree := range snapshotFixture(t, node).Worktrees {
				if tree.Project != nil && *tree.Project == project.ID && tree.Main {
					worktree = tree.ID
					break
				}
			}
			if worktree == (protocol.WorktreeID{}) {
				t.Fatal("project has no main worktree")
			}
			for _, test := range []struct {
				command protocol.Command
				allowed bool
				output  string
			}{
				{protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: worktree}}, workspace.services.Git, "git_status"},
				{protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: worktree}}, workspace.services.Worktrees, "git_worktrees"},
			} {
				if test.allowed {
					if out := commandFixture(t, node, test.command); out.Kind != test.output {
						t.Fatalf("%s produced %q, want %q", test.command.Kind, out.Kind, test.output)
					}
					continue
				}
				accepted, err := node.Dispatch(t.Context(), node.ID(), protocol.NewRequest(node.ID(), test.command))
				requireModuleUnavailable(t, err)
				if accepted.Completion != nil {
					t.Fatalf("cross-capability %s was admitted", test.command.Kind)
				}
			}
			stream, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "repository", Data: worktree})
			if stream != nil {
				defer stream.Close()
			}
			if workspace.services.Git {
				if err != nil || stream == nil {
					t.Fatalf("Git provider could not open repository subscription: %v", err)
				}
			} else {
				requireModuleUnavailable(t, err)
				if stream != nil {
					t.Fatal("worktree provider exposed a repository subscription")
				}
			}
		})
	}
}
