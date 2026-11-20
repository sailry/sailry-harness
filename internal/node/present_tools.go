package node

import (
	"context"
	"encoding/json"
	"mime"
	"path"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const presentFilesLimit = 16

const presentFilesDescription = "Deliver finished files to the user as file cards in this conversation, after creating and verifying them in the session worktree. Pass each deliverable's exact path: documents, spreadsheets, presentations, PDFs, standalone HTML pages, exports, archives, media or pictures, and source files only when the user asked for the file itself. A card refers to the file's version at this call; present a file again after changing it. Do not repeat a presented file as a link in the reply."

const presentFilesSchema = `{"type":"object","additionalProperties":false,"required":["paths"],"properties":{"paths":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"string","description":"Relative to the session worktree, with forward slashes"}}}}`

// presentTools offers the conversation's explicit delivery: each named
// worktree file becomes a file card and a published output. A delegated
// child reports to its parent, so it delivers nothing to the user.
func (node *Node) presentTools(inv *store.AgentInvocation) ([]engine.Tool, error) {
	if inv.Session.Delegation != nil && *inv.Session.Delegation != nil || inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil {
		return nil, nil
	}
	var schema map[string]any
	if err := json.Unmarshal([]byte(presentFilesSchema), &schema); err != nil {
		return nil, err
	}
	root := inv.Worktree.Path
	tool := engine.Tool{Definition: fantasy.FunctionTool{Name: operations.PresentFilesTool, Description: presentFilesDescription, InputSchema: schema},
		SchemaJSON: json.RawMessage(presentFilesSchema), ReadOnly: true, Concurrent: true}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		var arguments struct {
			Paths []string `json:"paths"`
		}
		if err := json.Unmarshal(call.Arguments, &arguments); err != nil || len(arguments.Paths) == 0 || len(arguments.Paths) > presentFilesLimit {
			return engine.ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "expected 1 to 16 worktree file paths"}}
		}
		files, err := presentFiles(ctx, root, arguments.Paths)
		if err != nil {
			return workspaceToolFault(err)
		}
		content := protocol.ToolContent{Version: 1}
		presented := make([]map[string]any, 0, len(files))
		for index := range files {
			content.Blocks = append(content.Blocks, protocol.ToolContentBlock{Kind: "file", Data: &files[index]})
			presented = append(presented, map[string]any{"path": files[index].Path, "size": *files[index].Size})
		}
		return engine.ToolResult{Value: map[string]any{"presented": presented, "sailry_content": content}}
	}
	return []engine.Tool{tool}, nil
}

// presentFiles describes regular files within the worktree, refusing the
// whole call when one is missing so that no partial delivery is shown.
func presentFiles(ctx context.Context, root string, paths []string) ([]protocol.ToolContentFile, error) {
	confined, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return nil, err
	}
	defer confined.Close()
	seen := map[string]bool{}
	files := make([]protocol.ToolContentFile, 0, len(paths))
	for _, relative := range paths {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		relative = strings.TrimPrefix(strings.TrimSpace(relative), "./")
		if seen[relative] {
			continue
		}
		seen[relative] = true
		file, err := confined.Open(relative)
		if err != nil {
			return nil, err
		}
		info, err := file.Stat()
		file.Close()
		if err != nil {
			return nil, err
		}
		revision, err := confined.SourceRevision(relative)
		if err != nil {
			return nil, err
		}
		media := mime.TypeByExtension(strings.ToLower(path.Ext(relative)))
		if media == "" {
			media = "application/octet-stream"
		}
		media, _, _ = strings.Cut(media, ";")
		size := uint64(info.Size())
		files = append(files, protocol.ToolContentFile{Path: relative, Mime: media, Size: &size, Revision: &revision})
	}
	return files, nil
}
