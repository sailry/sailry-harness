package plugins

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) worktree() (protocol.WorktreeID, error) {
	if worktree := owner.scope().Worktree; worktree != nil {
		return *worktree, nil
	}
	return protocol.WorktreeID{}, hostInvalid("plugin has no worktree scope")
}

func (owner *Host) workspaceOperation(kind string, input hostArguments, prepare, files bool) HostOperation {
	return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		worktree, err := owner.worktree()
		if err != nil {
			return nil, err
		}
		if files && owner.scope().Surface.Kind != "workspace" {
			return nil, hostInvalid("file operations require a workspace")
		}
		data := map[string]any{}
		if input != nil {
			value, err := input(arguments)
			if err != nil {
				return nil, err
			}
			var ok bool
			data, ok = value.(map[string]any)
			if !ok {
				return nil, hostInvalid("plugin host options must be an object")
			}
		}
		data["worktree"] = worktree
		if prepare {
			return owner.prepareData(ctx, kind, data)
		}
		return owner.readData(ctx, kind, data)
	}
}

func (owner *Host) addWorkspaceSDK(add hostSDKAdd) {
	for _, definition := range []struct {
		name, command string
		arguments     hostArguments
	}{
		{"inspectGit", "inspect_git", nil},
		{"listGitBranches", "list_git_branches", nil},
		{"resolveGitRevision", "resolve_git_revision", namedArguments("revision")},
		{"readGitDiff", "read_git_diff", namedArguments("path", "scope")},
		{"readGitCommit", "read_git_commit", namedArguments("commit")},
		{"readGitStash", "read_git_stash", namedArguments("commit")},
		{"readGitOutput", "read_git_output", nil},
		{"listGitRemoteTags", "list_git_remote_tags", namedArguments("remote")},
		{"readWorktreeCatalog", "read_worktree_catalog", nil},
		{"listWorktrees", "list_worktrees", nil},
		{"listTerminals", "list_terminals", nil},
		{"listTerminalTools", "list_terminal_tools", nil},
		{"outlineCode", "outline_code", namedArguments("path")},
		{"searchCode", "search_code", func(arguments []json.RawMessage) (any, error) {
			query, err := hostOptional(arguments, 0, map[string]json.RawMessage{})
			return map[string]any{"query": query}, err
		}},
	} {
		add(definition.name, true, owner.workspaceOperation(definition.command, definition.arguments, false, false))
	}
	add("readGitLog", true, owner.workspaceOperation("read_git_log", func(arguments []json.RawMessage) (any, error) {
		limit, err := hostValue[uint16](arguments, 0)
		if err != nil {
			return nil, err
		}
		cursor, err := hostOptional[json.RawMessage](arguments, 1, json.RawMessage("null"))
		if err != nil {
			return nil, err
		}
		options, err := hostOptional(arguments, 2, map[string]json.RawMessage{})
		if err != nil {
			return nil, err
		}
		return map[string]any{"limit": limit, "cursor": cursor, "revision": optionalRaw(options, "revision"), "path": optionalRaw(options, "path")}, nil
	}, false, false))
	add("prepareGitIndex", false, owner.workspaceOperation("update_git_index", firstArgument, true, false))
	add("prepareGitCommit", false, owner.workspaceOperation("create_git_commit", firstArgument, true, false))
	add("prepareGitChange", false, owner.prepareGitChange)
	add("prepareWorktreeChange", false, owner.prepareWorktreeChange)
	add("listDirectory", true, owner.workspaceOperation("list_directory", namedArgumentsWithDefaults(map[string]any{"after": nil}, "path", "after"), false, true))
	add("readFileLines", true, owner.workspaceOperation("read_file_lines", namedArgumentsWithDefaults(
		map[string]any{"offset": uint64(0), "limit": uint64(200), "expected_revision": nil}, "path", "offset", "limit", "expected_revision",
	), false, true))
	add("searchFiles", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		input, err := firstArgument(arguments)
		if err != nil {
			return nil, err
		}
		operation, cancel := context.WithCancel(ctx)
		defer cancel()
		owner.mu.Lock()
		if owner.fileSearchCancel != nil {
			owner.fileSearchCancel()
		}
		owner.fileSearchCancel = cancel
		owner.mu.Unlock()
		return owner.workspaceOperation("search_files", func([]json.RawMessage) (any, error) { return map[string]any{"options": input}, nil }, false, true)(operation, nil)
	})
	add("cancelFileSearch", false, func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		owner.mu.Lock()
		if owner.fileSearchCancel != nil {
			owner.fileSearchCancel()
			owner.fileSearchCancel = nil
		}
		owner.mu.Unlock()
		return json.RawMessage("null"), nil
	})
	add("prepareFileAction", false, owner.nativeOperation("sailry/sdk", "prepareFileAction"))
	add("listRoles", true, owner.readOperation("list_roles", nil))
	add("newRoleId", false, func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return hostJSON(protocol.NewRoleID().String())
	})
	add("prepareRole", false, owner.prepareOperation("put_role", namedArguments("role", "expected_revision")))
	add("prepareRemoveRole", false, owner.prepareOperation("remove_role", namedArguments("role", "expected_revision")))
	add("fetchWeb", true, owner.readOperation("fetch_web", firstArgument))
	add("readMediaSettings", true, owner.readOperation("read_media_settings", nil))
	add("listMediaModels", true, owner.readOperation("list_media_models", nil))
	add("prepareMediaSettings", false, owner.prepareOperation("save_media_settings", firstArgument))
	add("queryKnowledge", true, owner.readOperation("query_knowledge", firstArgument))
	add("listKnowledgeCollections", true, owner.readOperation("list_knowledge_collections", nil))
	add("prepareKnowledgeCollection", false, owner.prepareKnowledgeCollection)
	add("prepareKnowledgeRebuild", false, owner.prepareOperation("rebuild_knowledge_collection", namedArguments("id", "expected_revision")))
	add("prepareKnowledgeRemoval", false, owner.prepareOperation("delete_knowledge_collection", namedArguments("id", "expected_revision")))
	add("prepareTerminal", false, owner.prepareTerminal)
	add("prepareOpenTerminal", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		terminal, err := hostValue[protocol.TerminalID](arguments, 0)
		if err != nil {
			return nil, err
		}
		appearance, err := owner.terminalAppearance()
		if err != nil {
			return nil, err
		}
		return owner.prepareData(ctx, "open_terminal", map[string]any{"terminal": terminal, "viewport": hostViewport(), "appearance": appearance})
	})
	add("prepareCloseTerminal", false, owner.workspaceOperation("close_terminal", namedArguments("terminal"), true, false))
}

func (owner *Host) prepareGitChange(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	draft, err := hostValue[map[string]json.RawMessage](arguments, 0)
	if err != nil {
		return nil, err
	}
	kind := nativeProp(draft, "kind", "")
	delete(draft, "kind")
	commands := map[string]string{
		"create": "create_git_branch", "rename": "rename_git_branch", "delete": "delete_git_branch",
		"switch": "switch_git_branch", "merge": "merge_git_branch", "action": "run_git_action",
	}
	command := commands[kind]
	if command == "" {
		return nil, hostInvalid("invalid Git change")
	}
	body, _ := hostJSON(draft)
	return owner.workspaceOperation(command, firstArgument, true, false)(ctx, []json.RawMessage{body})
}

func (owner *Host) prepareWorktreeChange(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	draft, err := hostValue[map[string]json.RawMessage](arguments, 0)
	if err != nil {
		return nil, err
	}
	source, err := owner.worktree()
	if err != nil {
		return nil, err
	}
	kind := nativeProp(draft, "kind", "")
	delete(draft, "kind")
	command := map[string]string{"register": "register_worktree", "create": "create_worktree", "managed": "create_managed_worktree", "remove": "remove_worktree"}[kind]
	if command == "" {
		return nil, hostInvalid("invalid worktree change")
	}
	if kind == "managed" {
		draft["source"], _ = hostJSON(source)
	}
	return owner.prepareData(ctx, command, draft)
}

func (owner *Host) prepareKnowledgeCollection(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	draft, err := hostValue[protocol.KnowledgeDraft](arguments, 0)
	if err != nil {
		return nil, err
	}
	definition := protocol.KnowledgeDefinition{Name: draft.Name, Model: draft.Model, Globs: draft.Globs, Tags: draft.Tags, SharedWith: []string{}}
	body, _ := hostJSON(draft.Source.Data)
	switch draft.Source.Kind {
	case "worktree":
		worktree, err := owner.worktree()
		if err != nil {
			return nil, err
		}
		var source protocol.KnowledgeDraftSourceWorktreePayload
		if json.Unmarshal(body, &source) != nil {
			return nil, hostInvalid("invalid knowledge source")
		}
		definition.Source = protocol.KnowledgeSource{Kind: "worktree", Data: protocol.KnowledgeSourceWorktreePayload{Worktree: worktree, Path: source.Path}}
	case "storage":
		var source protocol.KnowledgeSourceStoragePayload
		if json.Unmarshal(body, &source) != nil {
			return nil, hostInvalid("invalid knowledge source")
		}
		definition.Source = protocol.KnowledgeSource{Kind: "storage", Data: source}
	default:
		return nil, hostInvalid("invalid knowledge source")
	}
	return owner.prepareData(ctx, "create_knowledge_collection", map[string]any{"definition": definition, "plugin": nil})
}

func hostViewport() protocol.TerminalViewport {
	return protocol.TerminalViewport{Columns: 80, Rows: 24}
}

func (owner *Host) terminalAppearance() (protocol.TerminalAppearance, error) {
	if owner.native.TerminalAppearance == nil {
		return protocol.TerminalAppearance{}, hostUnavailable("terminal creation requires an active view")
	}
	return owner.native.TerminalAppearance()
}

func (owner *Host) prepareTerminal(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	worktree, err := owner.worktree()
	if err != nil {
		return nil, err
	}
	appearance, err := owner.terminalAppearance()
	if err != nil {
		return nil, err
	}
	access, err := hostOptional(arguments, 1, protocol.ProcessAccessUnrestricted)
	if err != nil {
		return nil, err
	}
	launch := map[string]any{"access": access, "worktree": worktree, "viewport": hostViewport(), "appearance": appearance}
	if len(arguments) > 0 && string(arguments[0]) != "null" {
		return owner.prepareData(ctx, "open_tool_terminal", map[string]any{"tool": arguments[0], "launch": launch})
	}
	return owner.prepareData(ctx, "create_terminal", launch)
}
