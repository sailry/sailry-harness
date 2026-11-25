package node

import (
	"archive/zip"
	"context"
	"database/sql"
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	filesnode "github.com/sailry/sailry-harness/modules/files/node"
)

func trustPackageFixture(t *testing.T, node *Node, name, version, tools string) protocol.PluginInfo {
	t.Helper()
	source := t.TempDir()
	const entry = "dev.sailry.platform/host/main.js"
	if err := os.MkdirAll(filepath.Dir(filepath.Join(source, entry)), 0o700); err != nil {
		t.Fatal(err)
	}
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"` + version + `","extensions":{"dev.sailry.platform":{"api_version":"v1","instructions":"Package instructions.","actions":["browser.read"],"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["read"]},"tools":[` + tools + `]}}}`
	for path, value := range map[string]string{"plugin.json": manifest, entry: `export function read(input) { return {source: "package", input}; }`} {
		if err := os.WriteFile(filepath.Join(source, path), []byte(value), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	info, err := node.plugins.manager.Install(node.pluginContext(t.Context()), source, "", name)
	if err != nil || info.Extension == nil || info.Extension.Host == nil || len(info.Issues) != 0 {
		t.Fatalf("fixture %s: %+v, %v", name, info, err)
	}
	return info
}

func recordTrustPackage(t *testing.T, node *Node, ctx context.Context, info protocol.PluginInfo, revision uint64) protocol.PluginInfo {
	t.Helper()
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(ctx, db, info, revision, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return info
}

func installLegacyFilesFixture(t *testing.T, node *Node, revision uint64) protocol.PluginInfo {
	t.Helper()
	archive, err := zip.OpenReader("plugins/testdata/official-legacy.zip")
	if err != nil {
		t.Fatal(err)
	}
	defer archive.Close()
	legacyFiles, err := fs.Sub(archive, "files")
	if err != nil {
		t.Fatal(err)
	}
	directory := t.TempDir()
	if err := os.CopyFS(directory, legacyFiles); err != nil {
		t.Fatal(err)
	}
	ctx := node.pluginContext(t.Context())
	info, err := node.plugins.manager.Install(ctx, directory, "", "files")
	const digest = "ac133d0ee39e44e1dfc86fdd42656b57258220180c19c55e193ed0e2d1f7649e"
	if err != nil || info.Summary.Digest != digest || !plugins.IsLegacyOfficialPackage("files", digest) {
		t.Fatalf("real historical package was not recognized: %s, %v", info.Summary.Digest, err)
	}
	return recordTrustPackage(t, node, ctx, info, revision)
}

func TestNativeFilesReplacementUsesCapturedDigest(t *testing.T) {
	t.Parallel()
	node := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{filesnode.Module()}})
	ctx := node.pluginContext(t.Context())
	thirdParty := trustPackageFixture(t, node, "files", "0.1.32", `{"name":"read_file","description":"Package-owned read","initial":true,"handler":{"name":"read","read_only":true,"parameters":{"type":"object"}}}`)
	thirdParty = recordTrustPackage(t, node, ctx, thirdParty, 0)
	owner := plugins.PackageReference(thirdParty)
	alias := operations.Alias(owner.Name, "read_file")
	session := sessionFixture(t, node)
	inv := &store.AgentInvocation{Session: session, Caller: node.ID(), Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
	inv.Run.Worktree, inv.Run.Turn = session.Worktree, protocol.NewTurnID()
	inv.Worktree.ID, inv.Turn.Config, inv.Turn.Plugins = session.Worktree, session.Config, []protocol.PluginReference{owner}
	inv.Instructions = []store.AgentInstruction{{Text: "Package instructions.", AnyTools: []string{alias}}}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		inv.User, err = store.LocalUser(ctx, db)
		inv.StorageUser = inv.User.ID
		return err
	}); err != nil {
		t.Fatal(err)
	}
	lookup := func(tools []engine.Tool, name string) *engine.Tool {
		if index := slices.IndexFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == name }); index >= 0 {
			return &tools[index]
		}
		return nil
	}
	tools, err := node.workspaceTools(ctx, inv)
	if err != nil {
		t.Fatal(err)
	}
	tool := lookup(tools, alias)
	if tool == nil || tool.Binding == nil || tool.Binding.Owner != owner || tool.Prepare != nil || tool.Execute == nil {
		t.Fatal("same-name package lost its own tool binding")
	}
	result := tool.Execute(ctx, canonical.ToolRequest{ID: "owned-read", Name: alias, Arguments: json.RawMessage(`{}`)})
	if value, ok := result.Value.(map[string]any); !ok || result.Failure != nil || value["source"] != "package" {
		t.Fatalf("same-name tool did not run its own script: %+v", result)
	}
	if native := lookup(tools, "read_file"); native == nil || native.LegacyAlias() != "" {
		t.Fatal("same-name package lost its alias to the native tool")
	}
	if len(inv.Instructions) != 1 || inv.Instructions[0].Text != "Package instructions." {
		t.Fatalf("same-name package lost its instructions: %+v", inv.Instructions)
	}

	historical := installLegacyFilesFixture(t, node, thirdParty.Summary.Revision)
	if !node.hasReplacedPlugin("files", historical.Summary.Digest) ||
		plugins.IsLegacyOfficialPackage("git", historical.Summary.Digest) || plugins.IsLegacyOfficialPackage("files", owner.Digest) {
		t.Fatal("the historical allowlist did not require both package name and digest")
	}
	inv.Turn.Plugins[0] = plugins.PackageReference(historical)
	tools, err = node.workspaceTools(ctx, inv)
	if err != nil {
		t.Fatal(err)
	}
	if native := lookup(tools, "read_file"); native == nil || native.LegacyAlias() != alias || lookup(tools, alias) != nil {
		t.Fatal("real historical package lost its native replacement alias")
	}
	if len(inv.Instructions) != 0 {
		t.Fatal("historical instructions duplicated the native replacement")
	}
}

func TestThirdPartyScriptCanOwnFormerCommandAlias(t *testing.T) {
	t.Parallel()
	node := serviceTestNode(t, moduleapi.Assembly{})
	info := trustPackageFixture(t, node, "commands", "0.1.23", `{"name":"write_command","description":"Package-only action","handler":{"name":"read","parameters":{"type":"object"}}}`)
	inv := &store.AgentInvocation{Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
	tools, err := node.scriptTools(t.Context(), inv, info, nil)
	if err != nil || len(tools) != 1 || tools[0].Binding == nil || tools[0].Binding.Owner != plugins.PackageReference(info) || tools[0].Prepare != nil {
		t.Fatalf("same-name command tool did not retain package authority: %d tools, %v", len(tools), err)
	}
}

func workspaceOperationFixture(t *testing.T, name, extension string) (*Node, *store.AgentInvocation) {
	t.Helper()
	node := serviceTestNode(t, moduleapi.Assembly{})
	ctx := node.pluginContext(t.Context())
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"1.0.0","extensions":{"dev.sailry.platform":` + extension + `}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := node.plugins.manager.Install(ctx, source, "", name)
	if err != nil || info.Extension == nil || len(info.Issues) != 0 {
		t.Fatalf("fixture %s: %+v, %v", name, info.Issues, err)
	}
	info = recordTrustPackage(t, node, ctx, info, 0)
	session := sessionFixture(t, node)
	inv := &store.AgentInvocation{Session: session, Caller: node.ID(), Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
	inv.Run.Worktree, inv.Run.Turn = session.Worktree, protocol.NewTurnID()
	inv.Worktree.ID, inv.Turn.Config, inv.Turn.Plugins = session.Worktree, session.Config, []protocol.PluginReference{plugins.PackageReference(info)}
	return node, inv
}

func TestWorkspaceCommandOperationsKeepCapturedAuthority(t *testing.T) {
	t.Parallel()
	node, inv := workspaceOperationFixture(t, "commands", `{"api_version":"v1","actions":["files.read","process.control"],"tools":[
		{"name":"run_command","operation":"files.read"},
		{"name":"write_command","operation":"process.write"},
		{"name":"stop_command","operation":"process.stop","contexts":["ssh"]}
	]}`)
	ctx := node.pluginContext(t.Context())
	tools, err := node.workspaceTools(ctx, inv)
	if err != nil {
		t.Fatal(err)
	}
	owner := inv.Turn.Plugins[0]
	for _, row := range []struct {
		name      string
		operation protocol.ToolOperation
		approval  bool
	}{
		{"run_command", protocol.ToolOperationReadFile, false},
		{"write_command", protocol.ToolOperationWriteCommand, true},
	} {
		alias := operations.Alias(owner.Name, row.name)
		index := slices.IndexFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == alias })
		if index < 0 {
			t.Fatalf("third-party operation %s was suppressed", alias)
		}
		tool := tools[index]
		if tool.Binding == nil || tool.Binding.Owner != owner || tool.Binding.Tool != alias || tool.Binding.Operation == nil ||
			*tool.Binding.Operation != row.operation || tool.Approval != row.approval || tool.LegacyAlias() != "" {
			t.Fatalf("package operation %s acquired the wrong authority: %+v", alias, tool.Binding)
		}
	}
	for _, name := range []string{"run_command", "read_command", "write_command", "stop_command"} {
		index := slices.IndexFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == name })
		if index < 0 || tools[index].LegacyAlias() != "" {
			t.Fatalf("the built-in %s disappeared or claimed the captured package's alias", name)
		}
	}
	stop := operations.Alias(owner.Name, "stop_command")
	if slices.ContainsFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == stop }) {
		t.Fatal("an operation outside the captured context was exposed")
	}
	inv.Turn.Plugins = nil
	tools, err = node.workspaceTools(ctx, inv)
	if err != nil || !slices.ContainsFunc(tools, func(tool engine.Tool) bool {
		return tool.Definition.Name == "stop_command" && tool.LegacyAlias() == stop
	}) {
		t.Fatalf("the absent package no longer permits historical alias migration: %v", err)
	}
}

func TestWorkspaceProgressAliasRemainsReserved(t *testing.T) {
	t.Parallel()
	for _, operation := range []string{"progress.update", "files.read"} {
		t.Run(operation, func(t *testing.T) {
			node, inv := workspaceOperationFixture(t, "progress", `{"api_version":"v1","actions":["files.read"],"tools":[
				{"name":"update_plan","operation":"`+operation+`","presentation":"progress"},
				{"name":"read_log","operation":"files.read"}
			]}`)
			tools, err := node.workspaceTools(node.pluginContext(t.Context()), inv)
			if err != nil {
				t.Fatal(err)
			}
			ordinary := operations.Alias("progress", "read_log")
			if !slices.ContainsFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == ordinary }) ||
				slices.ContainsFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == operations.LegacyUpdatePlanTool }) {
				t.Fatal("the historical plan alias must be reserved without suppressing other package tools")
			}
		})
	}
}

func TestScriptToolBindsOnlyItsCapturedOperation(t *testing.T) {
	t.Parallel()
	node := serviceTestNode(t, moduleapi.Assembly{})
	info := trustPackageFixture(t, node, "browser", "0.1.10", `{"name":"snapshot","description":"Package-owned snapshot","handler":{"name":"read","operation":"browser.read","parameters":{"type":"object"}}}`)
	ctx := node.pluginContext(t.Context())
	info = recordTrustPackage(t, node, ctx, info, 0)
	owner := plugins.PackageReference(info)
	alias := operations.Alias(owner.Name, "snapshot")
	inv := &store.AgentInvocation{Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
	inv.Turn.Plugins = []protocol.PluginReference{owner}
	for _, row := range []struct {
		name      string
		owner     protocol.PluginReference
		operation protocol.ToolOperation
		want      bool
	}{
		{"another digest", protocol.PluginReference{Name: owner.Name, Digest: strings.Repeat("e", 64)}, browserReadOperation, false},
		{"another operation", owner, browserControlOperation, false},
		{"same captured operation", owner, browserReadOperation, true},
	} {
		t.Run(row.name, func(t *testing.T) {
			native := engine.Tool{Definition: engine.FunctionTool(alias, "Fixture adapter", map[string]any{"type": "object"}),
				Binding: &store.ApprovalBinding{Owner: row.owner, Tool: alias, Operation: &row.operation},
				Execute: func(context.Context, canonical.ToolRequest) engine.ToolResult {
					return engine.ToolResult{Value: "fixture"}
				}}
			tools, err := node.pluginTools(ctx, inv, []engine.Tool{native})
			if err != nil {
				t.Fatal(err)
			}
			if !row.want {
				if len(tools) != 0 {
					t.Fatal("a foreign or mismatched adapter was offered")
				}
				return
			}
			if len(tools) != 1 || tools[0].Prepare == nil || tools[0].Binding != native.Binding {
				t.Fatal("the script did not keep its own declared adapter and approval binding")
			}
		})
	}
}

func TestNativeFeatureGatesIgnoreThirdPartyNames(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name, historical string
		check            func(nodeModuleHost, context.Context) error
		enable           func(context.Context, *sql.Conn) error
	}{
		{"external-browser", "6497ac89905032fcf6d52ef20a5117249dec20134bfd0e489f4dfd97673873a2", nodeModuleHost.checkExternalBrowserEnabled,
			func(ctx context.Context, db *sql.Conn) error {
				_, _, err := store.SaveExternalBrowserSettings(ctx, db, protocol.ExternalBrowserSettings{Enabled: true})
				return err
			}},
		{"computer", "54088aee4e65672c71ce0441db389a31183108949d292df0b5599913599bdb4b", nodeModuleHost.checkComputerEnabled,
			func(ctx context.Context, db *sql.Conn) error {
				_, _, err := store.SaveComputerSettings(ctx, db, protocol.ComputerSettings{Enabled: true})
				return err
			}},
	} {
		t.Run(row.name, func(t *testing.T) {
			node := moduleFixtureNode(t, row.name)
			host, ctx := nodeModuleHost{node: node}, node.pluginContext(t.Context())
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest = row.name, strings.Repeat("f", 64)
			info.Origin = &protocol.PluginOrigin{Kind: "official"}
			thirdParty := recordTrustPackage(t, node, ctx, info, 0)
			inv := &store.AgentInvocation{Turn: protocol.DefaultQueuedTurn()}
			inv.Turn.Plugins = []protocol.PluginReference{plugins.PackageReference(thirdParty)}
			if _, captured := host.PluginCaptured(inv, row.name); captured {
				t.Fatal("unrecognized digest became a native module capture")
			}
			requireModuleUnavailable(t, row.check(host, ctx))
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				var err error
				thirdParty, err = store.SetPluginEnabled(ctx, db, row.name, thirdParty.Summary.Revision, false)
				if err != nil {
					return err
				}
				return row.enable(ctx, db)
			}); err != nil {
				t.Fatal(err)
			}
			if err := row.check(host, ctx); err != nil {
				t.Fatalf("unrelated disabled package blocked the native setting: %v", err)
			}
			info.Summary.Digest = row.historical
			info.Origin = &protocol.PluginOrigin{Kind: "directory"}
			historical := recordTrustPackage(t, node, ctx, info, thirdParty.Summary.Revision)
			inv.Turn.Plugins[0] = plugins.PackageReference(historical)
			if captured, ok := host.PluginCaptured(inv, row.name); !ok || captured != inv.Turn.Plugins[0] {
				t.Fatal("exact historical digest lost its native module capture")
			}
			requireModuleUnavailable(t, row.check(host, ctx))
		})
	}
}

func TestNativeMemoryDoesNotBorrowThirdPartyStorage(t *testing.T) {
	t.Parallel()
	node := moduleFixtureNode(t, "memory")
	host, ctx := nodeModuleHost{node: node}, node.pluginContext(t.Context())
	scope, err := host.controllerStorageScope(ctx, store.NativeMemoryNamespace)
	if err != nil || scope.Package != store.NativeMemoryReference() {
		t.Fatalf("empty profile did not use the native storage identity: %+v, %v", scope.Package, err)
	}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest = store.NativeMemoryNamespace, strings.Repeat("f", 64)
	info = recordTrustPackage(t, node, ctx, info, 0)
	_, err = host.controllerStorageScope(ctx, store.NativeMemoryNamespace)
	requireModuleUnavailable(t, err)
	inv := &store.AgentInvocation{Turn: protocol.DefaultQueuedTurn()}
	inv.Turn.Plugins = []protocol.PluginReference{plugins.PackageReference(info)}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		inv.User, err = store.LocalUser(ctx, db)
		inv.StorageUser = inv.User.ID
		return err
	}); err != nil {
		t.Fatal(err)
	}
	_, _, err = host.storageScope(ctx, inv, store.NativeMemoryNamespace)
	requireModuleUnavailable(t, err)
}
