package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func ResolvePluginMessageCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, request protocol.Request) (*protocol.Request, error) {
	if request.Plugin != nil {
		return nil, nil
	}
	var sessionID protocol.SessionID
	var revision *uint64
	var message protocol.ConversationInput
	switch request.Command.Kind {
	case "submit_turn":
		input, err := providerPayload[protocol.CommandSubmitTurnPayload](request.Command)
		if err != nil {
			return nil, err
		}
		sessionID, revision, message = input.Session, &input.ExpectedRevision, input.Message
	case "submit_task":
		input, err := providerPayload[protocol.CommandSubmitTaskPayload](request.Command)
		if err != nil {
			return nil, err
		}
		sessionID, revision, message = input.Session, input.ExpectedRevision, input.Message
	default:
		return nil, nil
	}
	text, present := strings.CutPrefix(strings.TrimLeftFunc(message.Text, unicode.IsSpace), "/")
	if !present {
		return nil, nil
	}
	end := strings.IndexFunc(text, unicode.IsSpace)
	if end < 0 {
		end = len(text)
	}
	name := text[:end]
	session, err := ReadSession(ctx, db, sessionID)
	if err != nil {
		return nil, err
	}
	if revision != nil {
		if err := checkRevision(session.Revision, *revision); err != nil {
			return nil, err
		}
	}
	if session.Delegation != nil {
		return nil, fault(protocol.ErrorCodePermissionDenied, "child conversation is read-only")
	}
	packages, err := pluginMessageCommandPackages(ctx, db, session.Config.Assistant)
	if err != nil {
		return nil, err
	}
	var selected *protocol.Request
	for _, info := range packages {
		if info.Extension == nil || info.Extension.Host == nil {
			continue
		}
		for _, command := range info.Extension.Host.Commands {
			if command.Name != name {
				continue
			}
			if selected != nil {
				return nil, fault(protocol.ErrorCodeConflict, "slash command has multiple providers")
			}
			input, err := json.Marshal(struct {
				Arguments string                     `json:"arguments"`
				Message   protocol.ConversationInput `json:"message"`
			}{strings.TrimSpace(text[end:]), message})
			if err != nil {
				return nil, err
			}
			child := protocol.NewRequest(node, protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{
				Handler: command.Handler, Input: input,
			}})
			child.Plugin = &protocol.PluginContext{
				Surface: protocol.PluginDesktopSurface{Kind: "workspace"}, Package: plugins.PackageReference(info),
				Worktree: new(session.Worktree), Session: new(session.ID),
			}
			identity, err := json.Marshal([]any{request.ID, child.Plugin})
			if err != nil {
				return nil, err
			}
			var digest [32]byte
			blake3.DeriveKey(digest[:], "Sailry plugin command v1", identity)
			copy(child.ID[:], digest[:16])
			child.ID[6] = child.ID[6]&0x0f | 0x80
			child.ID[8] = child.ID[8]&0x3f | 0x80
			selected = &child
			break
		}
	}
	return selected, nil
}

func pluginMessageCommandPackages(ctx context.Context, db *sql.Conn, assistant *protocol.PluginConversationBinding) ([]protocol.PluginInfo, error) {
	if assistant != nil {
		return assistantPackages(ctx, db, *assistant)
	}
	inventory, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	packages := []protocol.PluginInfo{}
	for _, info := range inventory {
		if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" ||
			info.Extension != nil && info.Extension.Scope == protocol.PluginScopeDesktop ||
			len(info.Skills) == 0 && len(info.Mcp) == 0 && (info.Extension == nil || len(info.Extension.Tools) == 0) {
			continue
		}
		captured, err := ReadPluginVersion(ctx, db, plugins.PackageReference(info))
		if err != nil {
			return nil, err
		}
		packages = append(packages, captured)
	}
	return packages, nil
}
