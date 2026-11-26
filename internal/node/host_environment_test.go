package node

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func writeFakeToolBinary(t *testing.T, dir, name, stdoutLine, stderrLine string) string {
	t.Helper()
	if runtime.GOOS == "windows" {
		path := filepath.Join(dir, name+".cmd")
		body := "@echo off\r\n"
		if stdoutLine != "" {
			for line := range strings.SplitSeq(stdoutLine, "\n") {
				if line == "" {
					body += "echo.\r\n"
				} else {
					body += "echo " + line + "\r\n"
				}
			}
		}
		if stderrLine != "" {
			for line := range strings.SplitSeq(stderrLine, "\n") {
				if line == "" {
					body += "echo. 1>&2\r\n"
				} else {
					body += "echo " + line + " 1>&2\r\n"
				}
			}
		}
		if err := os.WriteFile(path, []byte(body), 0755); err != nil {
			t.Fatal(err)
		}
		return path
	}
	path := filepath.Join(dir, name)
	body := "#!/bin/sh\n"
	if stdoutLine != "" {
		body += "printf '%s\\n' '" + stdoutLine + "'\n"
	}
	if stderrLine != "" {
		body += "printf '%s\\n' '" + stderrLine + "' >&2\n"
	}
	if err := os.WriteFile(path, []byte(body), 0755); err != nil {
		t.Fatal(err)
	}
	return path
}

func fakeHostToolEnvironment(t *testing.T, binDir string) process.Environment {
	t.Helper()
	entries := []string{"PATH=" + binDir, "HOME=" + t.TempDir()}
	if runtime.GOOS == "windows" {
		entries = append(entries, "PATHEXT=.CMD;.EXE")
		if comspec, ok := os.LookupEnv("COMSPEC"); ok {
			entries = append(entries, "COMSPEC="+comspec)
		}
		if systemRoot, ok := os.LookupEnv("SystemRoot"); ok {
			entries = append(entries, "SystemRoot="+systemRoot)
		}
	}
	return process.NewEnvironment(entries)
}

func hostToolByName(t *testing.T, tools []protocol.HostTool, name string) protocol.HostTool {
	t.Helper()
	for _, tool := range tools {
		if tool.Name == name {
			return tool
		}
	}
	t.Fatalf("host tool %q not found in %#v", name, tools)
	return protocol.HostTool{}
}

func startHostEnvTestNode(t *testing.T, binDir string, assembly moduleapi.Assembly) *Node {
	t.Helper()
	env := fakeHostToolEnvironment(t, binDir)
	owner, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		ProductID:   "harness",
		Headless:    true,
		Assembly:    assembly,
		Environment: &env,
		MCP:         fixtureMCPOptions(),
		Process: process.Options{
			After: func(time.Duration) <-chan time.Time { return make(chan time.Time) },
			Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
				return protocol.ProcessCompletion{}, errors.New("test must not launch workspace processes")
			},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := owner.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return owner
}

type stubCloneWorkspaceModule struct {
	sources map[string]moduleapi.ProjectSourcePreparer
}

func (m stubCloneWorkspaceModule) ID() string                { return "git" }
func (m stubCloneWorkspaceModule) ReplacesPlugins() []string { return nil }
func (m stubCloneWorkspaceModule) Catalog() []moduleapi.ToolMeta {
	return nil
}
func (m stubCloneWorkspaceModule) ToolsForTurn(context.Context, moduleapi.Host, *store.AgentInvocation) ([]engine.Tool, error) {
	return nil, nil
}
func (m stubCloneWorkspaceModule) WorkspaceServices() moduleapi.WorkspaceServices {
	return moduleapi.WorkspaceServices{
		Git: true,
		Bind: func(*moduleapi.WorkspaceRuntime) (moduleapi.WorkspaceBindings, error) {
			return moduleapi.WorkspaceBindings{ProjectSources: m.sources}, nil
		},
	}
}

func TestHostEnvironmentProbeDeterministic(t *testing.T) {
	t.Parallel()

	t.Run("empty PATH reports all canonical tools unavailable without OS PATH leakage", func(t *testing.T) {
		t.Parallel()
		emptyDir := t.TempDir()
		tools := probeHostTools(t.Context(), fakeHostToolEnvironment(t, emptyDir))
		wantNames := []string{"fd", "git", "node", "python", "rg", "uv"}
		if len(tools) != len(wantNames) {
			t.Fatalf("len(tools) = %d, want %d", len(tools), len(wantNames))
		}
		for i, name := range wantNames {
			if tools[i].Name != name || tools[i].Available || tools[i].Path != nil || tools[i].Version != nil {
				t.Fatalf("tools[%d] = %+v, want unavailable %q", i, tools[i], name)
			}
		}
	})

	t.Run("resolves fake binaries, prefers python3 over python, and keeps first line of version", func(t *testing.T) {
		t.Parallel()
		binDir := t.TempDir()
		gitPath := writeFakeToolBinary(t, binDir, "git", "git version 2.99.0", "")
		py3Path := writeFakeToolBinary(t, binDir, "python3", "Python 3.13.1", "")
		writeFakeToolBinary(t, binDir, "python", "Python 2.7.18", "")
		rgPath := writeFakeToolBinary(t, binDir, "rg", "ripgrep 14.2.0\n\nfeatures:+simd", "")
		if runtime.GOOS != "windows" {
			// Non-executable file must be ignored.
			if err := os.WriteFile(filepath.Join(binDir, "uv"), []byte("#!/bin/sh\necho uv 0.9.0\n"), 0644); err != nil {
				t.Fatal(err)
			}
		}

		tools := probeHostTools(t.Context(), fakeHostToolEnvironment(t, binDir))
		gitTool := hostToolByName(t, tools, "git")
		if !gitTool.Available || gitTool.Path == nil || *gitTool.Path != gitPath || gitTool.Version == nil || *gitTool.Version != "git version 2.99.0" {
			t.Fatalf("git tool = %+v, want path=%q version=%q", gitTool, gitPath, "git version 2.99.0")
		}
		pyTool := hostToolByName(t, tools, "python")
		if !pyTool.Available || pyTool.Path == nil || *pyTool.Path != py3Path || pyTool.Version == nil || *pyTool.Version != "Python 3.13.1" {
			t.Fatalf("python tool = %+v, want python3 path=%q version=%q", pyTool, py3Path, "Python 3.13.1")
		}
		rgTool := hostToolByName(t, tools, "rg")
		if !rgTool.Available || rgTool.Path == nil || *rgTool.Path != rgPath || rgTool.Version == nil || *rgTool.Version != "ripgrep 14.2.0" {
			t.Fatalf("rg tool = %+v, want first line version=%q", rgTool, "ripgrep 14.2.0")
		}
		for _, absent := range []string{"fd", "node", "uv"} {
			tool := hostToolByName(t, tools, absent)
			if tool.Available || tool.Path != nil || tool.Version != nil {
				t.Fatalf("%s = %+v, want unavailable", absent, tool)
			}
		}
	})

	t.Run("python falls back to python binary and captures stderr version", func(t *testing.T) {
		t.Parallel()
		binDir := t.TempDir()
		pyPath := writeFakeToolBinary(t, binDir, "python", "", "Python 3.11.9")
		tools := probeHostTools(t.Context(), fakeHostToolEnvironment(t, binDir))
		pyTool := hostToolByName(t, tools, "python")
		if !pyTool.Available || pyTool.Path == nil || *pyTool.Path != pyPath || pyTool.Version == nil || *pyTool.Version != "Python 3.11.9" {
			t.Fatalf("python fallback = %+v, want path=%q version=%q", pyTool, pyPath, "Python 3.11.9")
		}
	})
}

func TestHostSnapshotProjectSourcesAndInspectHost(t *testing.T) {
	t.Parallel()

	t.Run("harness snapshot emits only local project source and caches host tools until inspect_host refreshes", func(t *testing.T) {
		t.Parallel()
		binDir := t.TempDir()
		py3Path := writeFakeToolBinary(t, binDir, "python3", "Python 3.12.8", "")
		owner := startHostEnvTestNode(t, binDir, hostassembly.Harness())

		snap, err := owner.Snapshot(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(snap.ProjectSources, []string{"local"}) {
			t.Fatalf("Harness Snapshot.ProjectSources = %v, want [local]", snap.ProjectSources)
		}
		if gitTool := hostToolByName(t, snap.HostTools, "git"); gitTool.Available || gitTool.Path != nil {
			t.Fatalf("Harness Snapshot git = %+v, want unavailable", gitTool)
		}
		if pyTool := hostToolByName(t, snap.HostTools, "python"); !pyTool.Available || pyTool.Path == nil || *pyTool.Path != py3Path || pyTool.Version == nil || *pyTool.Version != "Python 3.12.8" {
			t.Fatalf("Harness Snapshot python = %+v, want python3", pyTool)
		}

		// Snapshot command returns the same fields as node.Snapshot.
		snapResp := dispatchFixture(t, owner, owner.ID(), protocol.NewRequest(owner.ID(), protocol.Command{Kind: "snapshot"}))
		if snapResp.Fault != nil {
			t.Fatal(snapResp.Fault)
		}
		cmdSnap := snapResp.Output.Data.(protocol.Snapshot)
		if !reflect.DeepEqual(cmdSnap.ProjectSources, []string{"local"}) || !reflect.DeepEqual(cmdSnap.HostTools, snap.HostTools) {
			t.Fatalf("snapshot command mismatch:\ngot:  sources=%v tools=%#v\nwant: sources=[local] tools=%#v", cmdSnap.ProjectSources, cmdSnap.HostTools, snap.HostTools)
		}

		// Install fake git into the existing PATH directory: cached Snapshot still
		// reports git absent until inspect_host triggers a fresh probe.
		gitPath := writeFakeToolBinary(t, binDir, "git", "git version 2.48.1", "")
		cachedSnap, err := owner.Snapshot(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		if gitTool := hostToolByName(t, cachedSnap.HostTools, "git"); gitTool.Available {
			t.Fatalf("expected cached snapshot before inspect_host refresh, got git=%+v", gitTool)
		}

		inspectResp := dispatchFixture(t, owner, owner.ID(), protocol.NewRequest(owner.ID(), protocol.Command{Kind: "inspect_host"}))
		if inspectResp.Fault != nil {
			t.Fatal(inspectResp.Fault)
		}
		info := inspectResp.Output.Data.(protocol.HostInfo)
		if gitTool := hostToolByName(t, info.Tools, "git"); !gitTool.Available || gitTool.Path == nil || *gitTool.Path != gitPath || gitTool.Version == nil || *gitTool.Version != "git version 2.48.1" {
			t.Fatalf("inspect_host git = %+v, want path=%q version=%q", gitTool, gitPath, "git version 2.48.1")
		}
		if pyTool := hostToolByName(t, info.Tools, "python"); !pyTool.Available || pyTool.Path == nil || *pyTool.Path != py3Path {
			t.Fatalf("inspect_host python = %+v, want path=%q", pyTool, py3Path)
		}

		// Subsequent Snapshot reflects the cache refreshed by inspect_host.
		refreshedSnap, err := owner.Snapshot(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		if gitTool := hostToolByName(t, refreshedSnap.HostTools, "git"); !gitTool.Available || gitTool.Path == nil || *gitTool.Path != gitPath {
			t.Fatalf("refreshed snapshot git = %+v, want available", gitTool)
		}
	})

	t.Run("node with clone project source emits local and clone in snapshot", func(t *testing.T) {
		t.Parallel()
		binDir := t.TempDir()
		gitPath := writeFakeToolBinary(t, binDir, "git", "git version 2.47.0", "")
		assembly := hostassembly.Harness()
		assembly.Modules = append(assembly.Modules, stubCloneWorkspaceModule{
			sources: map[string]moduleapi.ProjectSourcePreparer{
				"clone": func(_ context.Context, path string, _ protocol.ProjectsSource) (string, error) {
					return path, nil
				},
			},
		})
		owner := startHostEnvTestNode(t, binDir, assembly)

		snap, err := owner.Snapshot(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(snap.ProjectSources, []string{"local", "clone"}) {
			t.Fatalf("Code-like Snapshot.ProjectSources = %v, want [local clone]", snap.ProjectSources)
		}
		if gitTool := hostToolByName(t, snap.HostTools, "git"); !gitTool.Available || gitTool.Path == nil || *gitTool.Path != gitPath {
			t.Fatalf("Code-like Snapshot git = %+v, want path=%q", gitTool, gitPath)
		}
	})
}
