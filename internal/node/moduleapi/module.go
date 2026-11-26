// Package moduleapi defines the compile-time contract between the Node runtime
// and optional native feature modules under modules/*/node.
//
// Core packages never import concrete feature modules; an explicit assembly
// package (such as internal/assembly/host) constructs an Assembly and passes it
// to node.StartWithOptions.
package moduleapi

import (
	"context"
	"database/sql"
	"encoding/json"
	"io/fs"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ToolMeta describes presentation, grouping, localized display metadata and
// legacy package-alias resolution for one native module tool.
type ToolMeta struct {
	Name          string                    `json:"name"`
	LegacyPackage string                    `json:"legacy_package,omitempty"`
	LegacyServer  string                    `json:"legacy_server,omitempty"`
	LegacyName    string                    `json:"legacy_name,omitempty"`
	Presentation  protocol.ToolPresentation `json:"presentation,omitempty"`
	Grouping      protocol.ToolGrouping     `json:"grouping,omitempty"`
	Display       *protocol.ToolDisplay     `json:"display,omitempty"`
}

// LegacyAlias returns the canonical alias formerly generated when this tool was
// owned by a bundled plugin package (or plugin MCP server), or an empty string
// if none applies.
func (m ToolMeta) LegacyAlias() string {
	if m.LegacyPackage == "" {
		return ""
	}
	name := m.LegacyName
	if name == "" {
		name = m.Name
	}
	if m.LegacyServer != "" {
		return mcp.Alias(m.LegacyPackage, m.LegacyServer, name)
	}
	return operations.Alias(m.LegacyPackage, name)
}

// ToolSpec holds the declarative metadata of one native module tool, typically
// embedded as JSON in the module package.
type ToolSpec struct {
	Name          string                                `json:"name"`
	LegacyPackage string                                `json:"legacy_package,omitempty"`
	LegacyServer  string                                `json:"legacy_server,omitempty"`
	LegacyName    string                                `json:"legacy_name,omitempty"`
	Description   string                                `json:"description"`
	Operation     protocol.ToolOperation                `json:"operation,omitempty"`
	Action        string                                `json:"action,omitempty"`
	Parameters    json.RawMessage                       `json:"parameters"`
	Initial       bool                                  `json:"initial,omitempty"`
	ReadOnly      bool                                  `json:"read_only,omitempty"`
	Concurrent    bool                                  `json:"concurrent,omitempty"`
	StreamRead    bool                                  `json:"stream_read,omitempty"`
	Contexts      []protocol.PluginConversationResource `json:"contexts,omitempty"`
	Platforms     []string                              `json:"platforms,omitempty"`
	Presentation  protocol.ToolPresentation             `json:"presentation,omitempty"`
	Grouping      protocol.ToolGrouping                 `json:"grouping,omitempty"`
	Display       *protocol.ToolDisplay                 `json:"display,omitempty"`
}

// CatalogSpec pairs optional module-level instructions with its tool specs.
type CatalogSpec struct {
	Instructions string     `json:"instructions,omitempty"`
	Tools        []ToolSpec `json:"tools"`
}

// LoadCatalog decodes an embedded module tool catalog and fills default policy,
// presentation and grouping values.
func LoadCatalog(raw []byte) (CatalogSpec, error) {
	var catalog CatalogSpec
	if err := json.Unmarshal(raw, &catalog); err != nil {
		return catalog, err
	}
	for i := range catalog.Tools {
		tool := &catalog.Tools[i]
		if tool.Operation != "" && !tool.ReadOnly && !tool.Concurrent {
			tool.ReadOnly, tool.Concurrent = operations.Policy(tool.Operation)
		}
		if tool.Presentation == "" {
			tool.Presentation = protocol.ToolPresentationSummary
		}
		if tool.Grouping == "" {
			tool.Grouping = protocol.ToolGroupingSequence
		}
	}
	return catalog, nil
}

// Available reports whether this tool can be offered in the captured turn.
func (s ToolSpec) Available(inv *store.AgentInvocation) bool {
	if len(s.Platforms) > 0 && !slices.Contains(s.Platforms, runtime.GOOS) {
		return false
	}
	if !protocol.ToolContextAvailable(s.Contexts, inv.Worktree.Project, inv.Turn.Config) {
		return false
	}
	if inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(s.Contexts) == 0 &&
		!strings.HasPrefix(string(s.Operation), "artifacts.") {
		return false
	}
	return true
}

// Meta converts this specification to its static presentation metadata.
func (s ToolSpec) Meta() ToolMeta {
	presentation := s.Presentation
	if presentation == "" {
		presentation = protocol.ToolPresentationSummary
	}
	grouping := s.Grouping
	if grouping == "" {
		grouping = protocol.ToolGroupingSequence
	}
	return ToolMeta{
		Name:          s.Name,
		LegacyPackage: s.LegacyPackage,
		LegacyServer:  s.LegacyServer,
		LegacyName:    s.LegacyName,
		Presentation:  presentation,
		Grouping:      grouping,
		Display:       s.Display,
	}
}

// SearchLabels returns sorted, deduplicated display labels for tool_search.
func (s ToolSpec) SearchLabels() []string {
	if s.Display == nil {
		return nil
	}
	labels := []string{s.Display.Label}
	for _, label := range s.Display.Locales {
		labels = append(labels, label)
	}
	slices.Sort(labels)
	return slices.Compact(labels)
}

// LegacyAlias returns the former plugin-scoped alias for this tool, if any.
func (s ToolSpec) LegacyAlias() string {
	return s.Meta().LegacyAlias()
}

// WorkspaceToolConfig configures one native tool backed by a workspace operation.
type WorkspaceToolConfig struct {
	// FileWritePaths declares that preparation preserves the file targets in the operation arguments.
	FileWritePaths bool
	// ArtifactPackage identifies the compile-time owner of a session artifact operation.
	ArtifactPackage protocol.PluginReference
	Spec            ToolSpec
	Instruction     string
	TurnInstruction prompt.Part
	ContextParts    []string
	PrepareArgs     func(ctx context.Context, args json.RawMessage) (json.RawMessage, error)
	FormatResult    func(ctx context.Context, args json.RawMessage, result engine.ToolResult) engine.ToolResult
}

// ScopedToolConfig configures one native tool backed by a scoped Node operation
// such as local model install/removal.
type ScopedToolConfig struct {
	Spec        ToolSpec
	Instruction string
	PrepareArgs func(ctx context.Context, args json.RawMessage) (json.RawMessage, error)
}

// ConnectionToolConfig configures one native tool backed by a database or SSH
// connection operation.
type ConnectionToolConfig struct {
	Spec            ToolSpec
	Instruction     string
	TurnInstruction prompt.Part
	Captured        []protocol.ConnectionResource
	PrepareArgs     func(ctx context.Context, args json.RawMessage) (json.RawMessage, error)
	FormatResult    func(ctx context.Context, preparedArgs json.RawMessage, result engine.ToolResult) engine.ToolResult
}

// BrowserToolConfig configures one built-in controller browser tool.
type BrowserToolConfig struct {
	Spec         ToolSpec
	Instruction  string
	PluginGate   string
	PrepareArgs  func(ctx context.Context, args json.RawMessage) (json.RawMessage, error)
	FormatResult func(ctx context.Context, result engine.ToolResult) engine.ToolResult
}

// ExternalBrowserToolConfig configures one managed Chromium browser tool.
type ExternalBrowserToolConfig struct {
	Spec         ToolSpec
	Instruction  string
	Close        func() error
	PrepareArgs  func(ctx context.Context, args json.RawMessage) (json.RawMessage, error)
	FormatResult func(ctx context.Context, result engine.ToolResult) engine.ToolResult
}

// Host exposes narrowly scoped Node execution services to native modules.
type Host interface {
	WorkspaceTool(inv *store.AgentInvocation, cfg WorkspaceToolConfig) (engine.Tool, error)
	ScopedTool(inv *store.AgentInvocation, cfg ScopedToolConfig) (engine.Tool, error)
	ConnectionTool(inv *store.AgentInvocation, cfg ConnectionToolConfig) (engine.Tool, error)
	BrowserTool(inv *store.AgentInvocation, cfg BrowserToolConfig) (engine.Tool, error)
	ExternalBrowserTool(inv *store.AgentInvocation, cfg ExternalBrowserToolConfig) (engine.Tool, error)

	HasCodeIndex() bool
	RunWorkspaceOperation(ctx context.Context, inv *store.AgentInvocation, op protocol.ToolOperation, args json.RawMessage) (protocol.Output, error)
	ListLocalModels(ctx context.Context, kind protocol.ModelsKind) ([]protocol.ModelsLocalModel, error)

	ConnectionCatalog(ctx context.Context, inv *store.AgentInvocation) (store.ConnectionCatalog, error)
	CaptureConnectionTargets(inv *store.AgentInvocation, targets []protocol.ConnectionResource)

	BrowserAvailable(inv *store.AgentInvocation) bool
	ExternalBrowserAvailable() bool
	ExternalBrowserEnabled(ctx context.Context) (bool, error)
	ExternalBrowserCloseOnCancel(ctx context.Context, session protocol.SessionID) func() error

	ComputerEnabled(ctx context.Context) (bool, error)
	ConnectComputerMCP(ctx context.Context, inv *store.AgentInvocation, catalog CatalogSpec) ([]engine.Tool, error)

	PluginCaptured(inv *store.AgentInvocation, name string) (protocol.PluginReference, bool)
	PluginInstalled(ctx context.Context, name string) (installed bool, enabled bool, err error)

	PluginStorageGet(ctx context.Context, inv *store.AgentInvocation, namespace, key string) (protocol.PluginStorageEntry, error)
	PluginStorageSearch(ctx context.Context, inv *store.AgentInvocation, namespace string, query protocol.PluginStorageSearch) (protocol.PluginStorageSearchPage, error)
	PluginStorageTransaction(ctx context.Context, inv *store.AgentInvocation, namespace string, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error)
	ConfigureStorageApproval(ctx context.Context, db *sql.Conn, turn protocol.TurnID, value protocol.ConversationApproval) (protocol.ConversationApproval, error)

	// Controller-scoped storage helpers for native module commands (outside an agent turn).
	StorageGet(ctx context.Context, namespace, key string) (protocol.PluginStorageEntry, error)
	StorageListKeys(ctx context.Context, namespace, prefix string, after *string, limit uint16) (protocol.PluginStoragePage, error)
	StorageSearch(ctx context.Context, namespace string, query protocol.PluginStorageSearch) (protocol.PluginStorageSearchPage, error)
	StorageTransaction(ctx context.Context, namespace string, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error)
	StorageDispatch(ctx context.Context, namespace string, action protocol.DispatchCommand) (protocol.DispatchOutput, error)
	StopDispatchTurn(ctx context.Context, namespace string, job protocol.JobID) error

	// Turn-scoped list/dispatch helpers for native module agent tools.
	PluginStorageListKeys(ctx context.Context, inv *store.AgentInvocation, namespace, prefix string, after *string, limit uint16) (protocol.PluginStoragePage, error)
	PluginStorageDispatch(ctx context.Context, inv *store.AgentInvocation, namespace string, action protocol.DispatchCommand) (protocol.DispatchOutput, error)

	LocalNodeID() protocol.NodeID
	Now() time.Time
	ProjectExists(ctx context.Context, id string) (bool, error)
	ReadProjectCatalog(ctx context.Context) (protocol.ProjectsCatalog, error)
	ListTeams(ctx context.Context) ([]protocol.TeamProfile, error)
	ResolveModel(ctx context.Context, model string, effort *protocol.Effort, config *protocol.SessionConfig) (protocol.SessionConfig, error)
	ReadProviderAccountHTTP(ctx context.Context, provider protocol.ProviderID, expectedRevision uint64, namespace string, resolve func(protocol.Authentication) (protocol.PluginModelAuthEndpoint, bool)) (protocol.Authentication, protocol.PluginHttpResponse, error)
	ForwardModuleCommand(ctx context.Context, target protocol.NodeID, command protocol.Command) (protocol.Output, error)
}

// BuiltInSkill describes a compile-time skill bundled inside a native module.
type BuiltInSkill struct {
	Package string
	Skill   protocol.PluginSkill
	Tree    fs.FS
}

// SkillProvider is optionally implemented by native modules that ship built-in
// skills (for example, web-search:research).
type SkillProvider interface {
	Skills() []BuiltInSkill
}

// CommandHandler is optionally implemented by native modules that own direct
// Node protocol commands (such as native memory catalog CRUD).
type CommandHandler interface {
	HandlesCommand(kind string) bool
	ExecuteCommand(ctx context.Context, host Host, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error)
}

// DispatchProvider is optionally implemented by native modules that own a
// persistent Node dispatch namespace (for example scheduled-tasks or reminders)
// so existing schedules and jobs continue running after legacy plugins retire.
type DispatchProvider interface {
	NativeDispatchNamespace() string
}

// Module is one compile-time native Node feature module.
type Module interface {
	ID() string
	ReplacesPlugins() []string
	Catalog() []ToolMeta
	ToolsForTurn(ctx context.Context, host Host, inv *store.AgentInvocation) ([]engine.Tool, error)
}

// Assembly selects the compile-time Node modules and optional services.
type Assembly struct {
	// SystemPrompt replaces the default product instructions for main and
	// delegated turns. An empty value selects the Harness default.
	SystemPrompt string
	// Nil and empty lists select no packages.
	BundledDefaults []plugins.DefaultPackage
	// BundledPackages is immutable and rooted at plugins/<name>, with plugins/catalog.json.
	BundledPackages fs.FS
	CodeIndex       codeindexapi.Factory
	// Databases overrides the selected database executor when non-nil.
	Databases DatabaseExecutor
	// SSH overrides the selected SSH provider's factory when non-nil.
	SSH SSHFactory
	// Terminal overrides the selected terminal provider's factory when non-nil.
	Terminal TerminalFactory
	// Deprecated: retained for source compatibility. Workspace modules must
	// register their own handlers through WorkspaceServiceProvider.
	LegacyWorkspaceModules bool
	Modules                []Module
}

// NativeSkillPackages returns the skill trees contributed by assembled modules.
func (a Assembly) NativeSkillPackages() []operations.NativeSkillPackage {
	byPkg := map[string]*operations.NativeSkillPackage{}
	var order []string
	for _, m := range a.Modules {
		provider, ok := m.(SkillProvider)
		if !ok {
			continue
		}
		for _, item := range provider.Skills() {
			if item.Package == "" || item.Tree == nil {
				continue
			}
			pkg, exists := byPkg[item.Package]
			if !exists {
				pkg = &operations.NativeSkillPackage{Package: item.Package, Tree: item.Tree}
				byPkg[item.Package] = pkg
				order = append(order, item.Package)
			}
			pkg.Skills = append(pkg.Skills, item.Skill)
		}
	}
	out := make([]operations.NativeSkillPackage, 0, len(order))
	for _, name := range order {
		out = append(out, *byPkg[name])
	}
	return out
}

// NativeSkills returns the protocol-facing metadata of all built-in module skills.
func (a Assembly) NativeSkills() []protocol.NativeSkill {
	var out []protocol.NativeSkill
	for _, m := range a.Modules {
		provider, ok := m.(SkillProvider)
		if !ok {
			continue
		}
		for _, item := range provider.Skills() {
			out = append(out, protocol.NativeSkill{Package: item.Package, Skill: item.Skill})
		}
	}
	return out
}

// CommandHandlerFor returns the assembled module owning the given protocol
// command kind, if any.
func (a Assembly) CommandHandlerFor(kind string) CommandHandler {
	for _, m := range a.Modules {
		if handler, ok := m.(CommandHandler); ok && handler.HandlesCommand(kind) {
			return handler
		}
	}
	return nil
}

// ReplacesPlugin reports whether any assembled module supersedes the tool
// declarations of the named legacy plugin package.
func (a Assembly) ReplacesPlugin(name string) bool {
	for _, m := range a.Modules {
		if slices.Contains(m.ReplacesPlugins(), name) {
			return true
		}
	}
	return false
}

// LookupTool resolves presentation metadata for a canonical module tool name or
// its legacy plugin/MCP alias.
func (a Assembly) LookupTool(name string) (ToolMeta, bool) {
	for _, m := range a.Modules {
		for _, meta := range m.Catalog() {
			if meta.Name == name || (meta.LegacyPackage != "" && meta.LegacyAlias() == name) {
				return meta, true
			}
			if meta.LegacyServer != "" {
				legacyName := meta.LegacyName
				if legacyName == "" {
					legacyName = meta.Name
				}
				if engine.MCPToolName(meta.LegacyServer, legacyName) == name {
					return meta, true
				}
			}
		}
	}
	return ToolMeta{}, false
}

// BuildCustomTool constructs an engine.Tool directly from a ToolSpec and custom
// execution hooks (used by modules such as code-index workflows, OCR listing,
// and memory).
func BuildCustomTool(spec ToolSpec, instruction string, execute func(context.Context, canonical.ToolRequest) engine.ToolResult) (engine.Tool, error) {
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	tool := engine.Tool{
		Definition:    engine.FunctionTool(spec.Name, spec.Description, schema),
		SchemaJSON:    spec.Parameters,
		ReadOnly:      spec.ReadOnly,
		Concurrent:    spec.Concurrent,
		StreamRead:    spec.StreamRead,
		Deferred:      !spec.Initial,
		Approval:      !spec.ReadOnly,
		LegacyPackage: spec.LegacyPackage,
		LegacyServer:  spec.LegacyServer,
		LegacyName:    spec.LegacyName,
		Instruction:   strings.TrimSpace(instruction),
		SearchLabels:  spec.SearchLabels(),
		Execute:       execute,
	}
	return tool, nil
}
