package node

import (
	"bytes"
	"context"
	"os/exec"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	hostToolProbeTimeout    = time.Second
	hostToolOutputLimit     = 4096
	hostToolVersionMaxBytes = 256
)

type hostToolSpec struct {
	name       string
	candidates []string
	versionArg []string
}

// standardHostTools defines the canonical CLI runtime environment entries
// exposed by Snapshot.HostTools and HostInfo.Tools, sorted by canonical name.
var standardHostTools = []hostToolSpec{
	{name: "fd", candidates: []string{"fd"}, versionArg: []string{"--version"}},
	{name: "git", candidates: []string{"git"}, versionArg: []string{"--version"}},
	{name: "node", candidates: []string{"node"}, versionArg: []string{"--version"}},
	{name: "python", candidates: []string{"python3", "python"}, versionArg: []string{"--version"}},
	{name: "rg", candidates: []string{"rg"}, versionArg: []string{"--version"}},
	{name: "uv", candidates: []string{"uv"}, versionArg: []string{"--version"}},
}

type hostEnvironment struct {
	probeMu sync.Mutex
	mu      sync.Mutex
	cached  []protocol.HostTool
	ready   bool
}

func (node *Node) projectSources() []string {
	extra := make([]string, 0, len(node.workspace.ProjectSources))
	for kind, prepare := range node.workspace.ProjectSources {
		if kind == "" || kind == "local" || prepare == nil {
			continue
		}
		extra = append(extra, kind)
	}
	slices.Sort(extra)
	sources := make([]string, 0, 1+len(extra))
	sources = append(sources, "local")
	sources = append(sources, extra...)
	return sources
}

func (node *Node) attachHostSnapshot(ctx context.Context, snapshot *protocol.Snapshot) {
	snapshot.ProjectSources = node.projectSources()
	snapshot.HostTools = node.hostTools(ctx)
}

func (node *Node) hostTools(ctx context.Context) []protocol.HostTool {
	node.hostEnv.mu.Lock()
	if node.hostEnv.ready {
		result := cloneHostTools(node.hostEnv.cached)
		node.hostEnv.mu.Unlock()
		return result
	}
	node.hostEnv.mu.Unlock()

	node.hostEnv.probeMu.Lock()
	defer node.hostEnv.probeMu.Unlock()

	node.hostEnv.mu.Lock()
	if node.hostEnv.ready {
		result := cloneHostTools(node.hostEnv.cached)
		node.hostEnv.mu.Unlock()
		return result
	}
	node.hostEnv.mu.Unlock()

	return node.probeAndCacheHostTools(ctx)
}

func (node *Node) refreshHostTools(ctx context.Context) []protocol.HostTool {
	node.hostEnv.probeMu.Lock()
	defer node.hostEnv.probeMu.Unlock()
	return node.probeAndCacheHostTools(ctx)
}

func (node *Node) probeAndCacheHostTools(ctx context.Context) []protocol.HostTool {
	if ctx == nil {
		ctx = context.Background()
	}
	probeCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	if node.ctx != nil {
		stop := context.AfterFunc(node.ctx, cancel)
		defer stop()
	}
	tools := probeHostTools(probeCtx, node.environment)
	if probeCtx.Err() == nil {
		node.hostEnv.mu.Lock()
		node.hostEnv.cached = cloneHostTools(tools)
		node.hostEnv.ready = true
		node.hostEnv.mu.Unlock()
	}
	return tools
}

func probeHostTools(ctx context.Context, env process.Environment) []protocol.HostTool {
	search, envValues := hostToolSearchEnvironment(env)
	results := make([]protocol.HostTool, len(standardHostTools))
	present := make([]int, 0, len(standardHostTools))
	for i, spec := range standardHostTools {
		tool := protocol.HostTool{Name: spec.name}
		for _, candidate := range spec.candidates {
			if path, err := program.Resolve(candidate, search); err == nil {
				resolved := path
				tool.Available = true
				tool.Path = &resolved
				present = append(present, i)
				break
			}
		}
		results[i] = tool
	}
	if len(present) == 0 || ctx.Err() != nil {
		return results
	}
	var wg sync.WaitGroup
	for _, index := range present {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			results[i].Version = probeToolVersion(ctx, *results[i].Path, standardHostTools[i].versionArg, envValues)
		}(index)
	}
	wg.Wait()
	return results
}

func hostToolSearchEnvironment(env process.Environment) (map[string]string, []string) {
	values := env.TerminalValues()
	search := make(map[string]string, 2)
	for _, entry := range values {
		name, value, ok := strings.Cut(entry, "=")
		if !ok || name == "" {
			continue
		}
		switch {
		case strings.EqualFold(name, "PATH"):
			search["PATH"] = value
		case strings.EqualFold(name, "PATHEXT"):
			search["PATHEXT"] = value
		}
	}
	return search, values
}

type limitedBuffer struct {
	buf   bytes.Buffer
	limit int
}

func (b *limitedBuffer) Write(p []byte) (int, error) {
	if remain := b.limit - b.buf.Len(); remain > 0 {
		if len(p) > remain {
			b.buf.Write(p[:remain])
		} else {
			b.buf.Write(p)
		}
	}
	return len(p), nil
}

func probeToolVersion(ctx context.Context, path string, args []string, env []string) *string {
	probeCtx, cancel := context.WithTimeout(ctx, hostToolProbeTimeout)
	defer cancel()
	command := exec.CommandContext(probeCtx, path, args...)
	childEnv := make([]string, 0, len(env)+1)
	childEnv = append(childEnv, env...)
	childEnv = append(childEnv, "GIT_TERMINAL_PROMPT=0")
	command.Env = childEnv
	var stdout, stderr limitedBuffer
	stdout.limit, stderr.limit = hostToolOutputLimit, hostToolOutputLimit
	command.Stdout = &stdout
	command.Stderr = &stderr
	command.WaitDelay = 200 * time.Millisecond
	cleanup, err := program.StartGroup(command)
	if err != nil {
		return nil
	}
	defer cleanup()
	if err := command.Wait(); err != nil {
		return nil
	}
	if line := firstNonEmptyLine(stdout.buf.String()); line != "" {
		return &line
	}
	if line := firstNonEmptyLine(stderr.buf.String()); line != "" {
		return &line
	}
	return nil
}

func firstNonEmptyLine(text string) string {
	for line := range strings.SplitSeq(text, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" {
			continue
		}
		if len(trimmed) > hostToolVersionMaxBytes {
			end := hostToolVersionMaxBytes
			for end > 0 && !utf8.RuneStart(trimmed[end]) {
				end--
			}
			trimmed = strings.TrimSpace(trimmed[:end])
		}
		return trimmed
	}
	return ""
}

func cloneHostTools(tools []protocol.HostTool) []protocol.HostTool {
	if len(tools) == 0 {
		return nil
	}
	out := make([]protocol.HostTool, len(tools))
	for i, tool := range tools {
		cloned := protocol.HostTool{
			Name:      tool.Name,
			Available: tool.Available,
		}
		if tool.Path != nil {
			path := *tool.Path
			cloned.Path = &path
		}
		if tool.Version != nil {
			version := *tool.Version
			cloned.Version = &version
		}
		out[i] = cloned
	}
	return out
}
