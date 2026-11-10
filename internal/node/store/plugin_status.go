package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ResolvePluginStatusScope(ctx context.Context, db *sql.Conn, scope protocol.PluginUiStatusScope) (protocol.PluginUiStatusScope, error) {
	if !scope.Valid() {
		return scope, invalid("status requires a worktree or session")
	}
	effective := scope
	if scope.Session != nil {
		session, err := ReadSession(ctx, db, *scope.Session)
		if err != nil {
			return effective, err
		}
		if scope.Worktree != nil && *scope.Worktree != session.Worktree {
			return effective, fault(protocol.ErrorCodeWrongTarget, "status session belongs to another worktree")
		}
		effective.Worktree = &session.Worktree
	}
	if effective.Worktree != nil {
		if _, err := ReadWorkspaceRoot(ctx, db, *effective.Worktree, false); err != nil {
			return effective, err
		}
	}
	return effective, nil
}

func ReadPluginStatusPackages(ctx context.Context, db *sql.Conn, scope protocol.PluginUiStatusScope) ([]protocol.PluginInfo, error) {
	assistant := ""
	if scope.Session != nil {
		session, err := ReadSession(ctx, db, *scope.Session)
		if err != nil {
			return nil, err
		}
		if session.Config.Assistant != nil {
			assistant = session.Config.Assistant.Package.Name
		}
	}
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.PluginInfo{}
	for _, info := range infos {
		extension := info.Extension
		if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || extension == nil ||
			extension.Scope != protocol.PluginScopeHost || extension.Host == nil || extension.Host.Status == nil ||
			!slices.ContainsFunc(extension.Ui, func(entry protocol.PluginUiContribution) bool { return entry.Slot == protocol.PluginUiSlotStatus }) ||
			assistant != "" && assistant != info.Summary.Name && (extension.Desktop == nil || !extension.Desktop.UiShared) {
			continue
		}
		result = append(result, info)
	}
	return result, nil
}

func PluginStatusEntries(info protocol.PluginInfo, states []protocol.PluginUiState) []protocol.PluginUiStatusEntry {
	entries := []protocol.PluginUiStatusEntry{}
	if info.Extension == nil || info.Extension.Host == nil || info.Extension.Host.Status == nil {
		return entries
	}
	for _, declaration := range info.Extension.Ui {
		if declaration.Slot != protocol.PluginUiSlotStatus {
			continue
		}
		state := protocol.DefaultPluginUiState()
		state.ID = declaration.ID
		for _, candidate := range states {
			if candidate.ID == declaration.ID {
				state = candidate
				break
			}
		}
		entry := protocol.PluginUiStatusEntry{Package: plugins.PackageReference(info), Declaration: declaration, State: state}
		if declaration.Kind == protocol.PluginUiKindButton {
			entry.Invoke = info.Extension.Host.Status.Invoke
		}
		entries = append(entries, entry)
	}
	return entries
}

func ValidatePluginStatusStates(info protocol.PluginInfo, value json.RawMessage) ([]protocol.PluginUiState, error) {
	var encoded bytes.Buffer
	if err := json.Compact(&encoded, value); err != nil {
		return nil, invalid("invalid Status output")
	}
	if encoded.Len() > protocol.PluginUiMaxStateBytes {
		return nil, invalid("Status output exceeds its byte limit")
	}
	var states []protocol.PluginUiState
	if encoded.Len() == 0 || encoded.Bytes()[0] != '[' || json.Unmarshal(encoded.Bytes(), &states) != nil {
		return nil, invalid("invalid Status states")
	}
	if len(states) > protocol.PluginUiMaxEntries {
		return nil, invalid("Status output exceeds its entry limit")
	}
	ids := map[string]bool{}
	for _, state := range states {
		if ids[state.ID] || info.Extension == nil || !slices.ContainsFunc(info.Extension.Ui, func(entry protocol.PluginUiContribution) bool {
			return entry.ID == state.ID && protocol.ValidPluginUiStatusState(state, entry)
		}) {
			return nil, invalid("Status output does not match its declarations")
		}
		ids[state.ID] = true
	}
	return states, nil
}
