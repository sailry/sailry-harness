package client

import "github.com/sailry/sailry-harness/internal/protocol"

type PluginCursors struct {
	Cursor        uint64
	Package       uint64
	Settled       uint64
	TerminalTools uint64
}

type pluginChangeCursors struct {
	revision uint64
	packages map[string]uint64
	settled  map[protocol.WorktreeID]uint64
	reads    [5]uint64
}

func (client *Client) PluginCursors(name string, actions []protocol.PluginAction, worktree *protocol.WorktreeID) PluginCursors {
	client.mu.RLock()
	defer client.mu.RUnlock()
	changes := &client.project.changes
	value := PluginCursors{Package: changes.packages[name], TerminalTools: changes.reads[4]}
	if client.project.snapshot != nil {
		value.Cursor = client.project.snapshot.Cursor
	}
	if worktree != nil {
		value.Settled = changes.settled[*worktree]
	}
	for _, action := range actions {
		var cursor uint64
		switch action {
		case protocol.PluginActionReadRoles:
			cursor = changes.reads[0]
		case protocol.PluginActionReadModels:
			cursor = changes.reads[1]
		case protocol.PluginActionReadWorktrees:
			cursor = changes.reads[2]
		case protocol.PluginActionReadTerminals:
			cursor = changes.reads[3]
		}
		value.Package = max(value.Package, cursor)
	}
	return value
}

func (changes *pluginChangeCursors) advance() error {
	if changes.revision == ^uint64(0) {
		return fault(protocol.ErrorCodeInternal, "plugin change revision exhausted")
	}
	changes.revision++
	return nil
}

func (changes *pluginChangeCursors) reset(snapshot protocol.Snapshot) error {
	if err := changes.advance(); err != nil {
		return err
	}
	changes.packages = make(map[string]uint64, len(snapshot.Plugins))
	for _, entry := range snapshot.Plugins {
		changes.packages[entry.Name] = changes.revision
	}
	changes.settled = make(map[protocol.WorktreeID]uint64, len(snapshot.Worktrees))
	for _, tree := range snapshot.Worktrees {
		changes.settled[tree.ID] = snapshot.Cursor
	}
	for index := range changes.reads {
		changes.reads[index] = changes.revision
	}
	return nil
}

func (changes *pluginChangeCursors) observe(event protocol.Event, cursor uint64, settled *protocol.WorktreeID, removed bool) error {
	if settled != nil {
		if removed {
			delete(changes.settled, *settled)
		} else {
			changes.settled[*settled] = cursor
		}
	}
	switch event.Kind {
	case "role_changed", "role_removed", "provider_changed", "provider_removed", "model_catalog_changed",
		"local_models_changed", "terminal_changed", "terminal_settings_changed", "worktree_registered", "worktree_removed",
		"plugin_changed", "plugin_values_changed", "plugin_removed":
		if err := changes.advance(); err != nil {
			return err
		}
	}
	switch event.Kind {
	case "role_changed", "role_removed":
		changes.reads[0] = changes.revision
	case "provider_changed", "provider_removed", "model_catalog_changed", "local_models_changed":
		changes.reads[1] = changes.revision
	case "terminal_settings_changed":
		changes.reads[3], changes.reads[4] = changes.revision, changes.revision
	case "terminal_changed":
		changes.reads[3] = changes.revision
	case "worktree_registered":
		changes.reads[2] = changes.revision
	case "worktree_removed":
		changes.reads[2], changes.reads[3] = changes.revision, changes.revision
	case "plugin_changed":
		entry, err := payload[protocol.PluginSummary](event.Data)
		if err != nil {
			return err
		}
		changes.packages[entry.Name] = changes.revision
	case "plugin_values_changed":
		entry, err := payload[protocol.EventPluginValuesChangedPayload](event.Data)
		if err != nil {
			return err
		}
		if _, exists := changes.packages[entry.Name]; exists {
			changes.packages[entry.Name] = changes.revision
		}
	case "plugin_removed":
		entry, err := payload[protocol.EventPluginRemovedPayload](event.Data)
		if err != nil {
			return err
		}
		delete(changes.packages, entry.Name)
	}
	return nil
}

func (projection *Projection) settledChange(event protocol.Event) (*protocol.WorktreeID, bool, error) {
	snapshot := projection.snapshot
	complete := func(sessionID protocol.SessionID) *protocol.WorktreeID {
		for _, session := range snapshot.Sessions {
			if session.ID == sessionID {
				worktree := session.Worktree
				if session.Activity.Run != nil {
					worktree = session.Activity.Run.Worktree
				}
				return &worktree
			}
		}
		return nil
	}
	switch event.Kind {
	case "tools_completed":
		value, err := payload[protocol.EventToolsCompletedPayload](event.Data)
		if err != nil {
			return nil, false, err
		}
		return complete(value.Session), false, nil
	case "conversation_changed":
		value, err := payload[protocol.EventConversationChangedPayload](event.Data)
		if err != nil {
			return nil, false, err
		}
		if value.Activity != nil && settledActivity(snapshot, value.Session, *value.Activity) {
			return complete(value.Session), false, nil
		}
	case "session_changed":
		value, err := payload[protocol.Session](event.Data)
		if err != nil {
			return nil, false, err
		}
		if settledActivity(snapshot, value.ID, value.Activity) {
			return &value.Worktree, false, nil
		}
	case "worktree_registered":
		value, err := payload[protocol.Worktree](event.Data)
		return &value.ID, false, err
	case "worktree_removed":
		value, err := payload[protocol.EventWorktreeRemovedPayload](event.Data)
		return &value.ID, true, err
	}
	return nil, false, nil
}

func settledActivity(snapshot *protocol.Snapshot, sessionID protocol.SessionID, activity protocol.ActivitySummary) bool {
	run := activity.Run
	if run == nil || run.Status == "queued" || run.Status == "running" || run.Status == "stopping" {
		return false
	}
	for _, session := range snapshot.Sessions {
		if session.ID == sessionID && session.Activity.Run != nil {
			previous := session.Activity.Run
			return previous.Turn != run.Turn || previous.Status != run.Status
		}
	}
	return true
}
