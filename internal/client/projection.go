package client

import (
	"bytes"
	"cmp"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Apply uint8

const (
	Applied Apply = iota
	Ignored
	Recover
)

// Projection reduces one Node's committed updates for one subscription generation.
type Projection struct {
	node       protocol.NodeID
	generation uint64
	snapshot   *protocol.Snapshot
	recovery   bool
	changes    pluginChangeCursors
}

func NewProjection(node protocol.NodeID, generation uint64) *Projection {
	return &Projection{node: node, generation: generation, recovery: true}
}

func (projection *Projection) Snapshot() *protocol.Snapshot {
	if projection.snapshot == nil {
		return nil
	}
	snapshot := copyValue(*projection.snapshot)
	return &snapshot
}

func (projection *Projection) Reconnect(generation uint64) error {
	if generation <= projection.generation {
		return fault(protocol.ErrorCodeInvalidRequest, "connection generation must increase")
	}
	projection.generation, projection.recovery = generation, true
	return nil
}

func (projection *Projection) Apply(generation uint64, update protocol.Update) (Apply, error) {
	if generation != projection.generation {
		return Ignored, nil
	}
	switch update.Kind {
	case "reset_required":
		projection.recovery = true
		return Recover, nil
	case "snapshot":
		snapshot, err := payload[protocol.Snapshot](update.Data)
		if err != nil {
			return Ignored, err
		}
		if snapshot.Node != projection.node {
			return Ignored, fault(protocol.ErrorCodeWrongTarget, "update belongs to another Node")
		}
		if !projection.recovery && projection.snapshot != nil && snapshot.Cursor < projection.snapshot.Cursor {
			return Ignored, nil
		}
		snapshot = copyValue(snapshot)
		retainQuotas(&snapshot)
		if err := projection.changes.reset(snapshot); err != nil {
			return Ignored, err
		}
		projection.snapshot, projection.recovery = &snapshot, false
		return Applied, nil
	case "event":
		envelope, err := payload[protocol.EventEnvelope](update.Data)
		if err != nil {
			return Ignored, err
		}
		if envelope.Node != projection.node {
			return Ignored, fault(protocol.ErrorCodeWrongTarget, "update belongs to another Node")
		}
		if projection.snapshot == nil {
			projection.recovery = true
			return Recover, nil
		}
		if envelope.Cursor <= projection.snapshot.Cursor {
			return Ignored, nil
		}
		if projection.recovery || projection.snapshot.Cursor == ^uint64(0) || projection.snapshot.Cursor+1 != envelope.Cursor {
			projection.recovery = true
			return Recover, nil
		}
		settled, removed, err := projection.settledChange(envelope.Event)
		if err != nil {
			return Ignored, err
		}
		recover, err := projection.reduce(copyValue(envelope.Event))
		if err != nil {
			return Ignored, err
		}
		if recover {
			projection.recovery = true
			return Recover, nil
		}
		if err := projection.changes.observe(envelope.Event, envelope.Cursor, settled, removed); err != nil {
			projection.recovery = true
			return Recover, err
		}
		projection.snapshot.Cursor = envelope.Cursor
		return Applied, nil
	default:
		return Ignored, fault(protocol.ErrorCodeInvalidRequest, "resource update on Node subscription")
	}
}

func (projection *Projection) reduce(event protocol.Event) (bool, error) {
	snapshot := projection.snapshot
	switch event.Kind {
	case "project_registered", "project_changed", "project_removed":
		// Registration also creates the main worktree in the same transaction.
		return true, nil
	case "session_changed":
		session, err := payload[protocol.Session](event.Data)
		if err != nil {
			return false, err
		}
		if session.Project == nil && !slices.ContainsFunc(snapshot.Worktrees, func(worktree protocol.Worktree) bool { return worktree.ID == session.Worktree }) {
			return true, nil
		}
		updateSession(snapshot, session)
	case "sessions_removed":
		ids, err := payload[[]protocol.SessionID](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Sessions = slices.DeleteFunc(snapshot.Sessions, func(session protocol.Session) bool { return slices.Contains(ids, session.ID) })
		snapshot.Turns = slices.DeleteFunc(snapshot.Turns, func(turn protocol.TurnSummary) bool { return slices.Contains(ids, turn.Session) })
	case "sessions_reordered":
		ids, err := payload[[]protocol.SessionID](event.Data)
		if err != nil {
			return false, err
		}
		positions := make(map[protocol.SessionID]int, len(ids))
		for index, id := range ids {
			positions[id] = index
		}
		if len(positions) != len(ids) || len(positions) != len(snapshot.Sessions) {
			return true, nil
		}
		for _, session := range snapshot.Sessions {
			if _, exists := positions[session.ID]; !exists {
				return true, nil
			}
		}
		slices.SortStableFunc(snapshot.Sessions, func(a, b protocol.Session) int { return cmp.Compare(positions[a.ID], positions[b.ID]) })
	case "session_rewound":
		change, err := payload[protocol.EventSessionRewoundPayload](event.Data)
		if err != nil {
			return false, err
		}
		if change.Session == nil {
			return false, fault(protocol.ErrorCodeInvalidRequest, "session update is missing its session")
		}
		updateSession(snapshot, *change.Session)
		if change.Backup != nil && *change.Backup != nil {
			updateSession(snapshot, **change.Backup)
		}
	case "defaults_changed":
		defaults, err := payload[protocol.Defaults](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Defaults = defaults
	case "worktree_registered":
		worktree, err := payload[protocol.Worktree](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Worktrees = slices.DeleteFunc(snapshot.Worktrees, func(item protocol.Worktree) bool { return item.ID == worktree.ID })
		snapshot.Worktrees = append(snapshot.Worktrees, worktree)
	case "worktree_removed":
		change, err := payload[protocol.EventWorktreeRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Worktrees = slices.DeleteFunc(snapshot.Worktrees, func(item protocol.Worktree) bool { return item.ID == change.ID })
		snapshot.Terminals = slices.DeleteFunc(snapshot.Terminals, func(item protocol.TerminalInfo) bool { return item.Worktree != nil && *item.Worktree == change.ID })
	case "conversation_changed":
		change, err := payload[protocol.EventConversationChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		if change.Activity == nil {
			return false, fault(protocol.ErrorCodeInvalidRequest, "conversation update is missing its activity")
		}
		for index := range snapshot.Sessions {
			if snapshot.Sessions[index].ID == change.Session {
				snapshot.Sessions[index].Activity = *change.Activity
				break
			}
		}
	case "task_started":
		change, err := payload[protocol.EventTaskStartedPayload](event.Data)
		if err != nil {
			return false, err
		}
		if change.Session == nil {
			return false, fault(protocol.ErrorCodeInvalidRequest, "task update is missing its session")
		}
		if !slices.ContainsFunc(snapshot.Worktrees, func(item protocol.Worktree) bool { return item.ID == change.Session.Worktree }) {
			return true, nil
		}
		updateSession(snapshot, *change.Session)
		updateTurn(snapshot, change.Turn)
	case "turn_queued":
		change, err := payload[protocol.EventTurnQueuedPayload](event.Data)
		if err != nil {
			return false, err
		}
		if change.Activity == nil {
			return false, fault(protocol.ErrorCodeInvalidRequest, "queued turn update is missing its activity")
		}
		for index := range snapshot.Sessions {
			if snapshot.Sessions[index].ID == change.Turn.Session {
				snapshot.Sessions[index].Activity = *change.Activity
			}
		}
		updateTurn(snapshot, change.Turn)
	case "plan_accepted":
		change, err := payload[protocol.ConversationQuestionAcceptedPlan](event.Data)
		if err != nil {
			return false, err
		}
		updateSession(snapshot, change.Session)
		updateTurn(snapshot, change.Turn)
	case "notification_changed":
		notice, err := payload[protocol.NotificationNotice](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Notifications = slices.DeleteFunc(snapshot.Notifications, func(item protocol.NotificationNotice) bool { return item.ID == notice.ID })
		snapshot.Notifications = append(snapshot.Notifications, notice)
		slices.SortStableFunc(snapshot.Notifications, func(a, b protocol.NotificationNotice) int { return cmp.Compare(b.Sequence, a.Sequence) })
		if len(snapshot.Notifications) > protocol.NotificationLimit {
			snapshot.Notifications = snapshot.Notifications[:protocol.NotificationLimit]
		}
	case "notifications_read":
		change, err := payload[protocol.EventNotificationsReadPayload](event.Data)
		if err != nil {
			return false, err
		}
		for index := range snapshot.Notifications {
			if slices.Contains(change.IDs, snapshot.Notifications[index].ID) {
				snapshot.Notifications[index].Read = true
			}
		}
	case "notifications_dismissed":
		change, err := payload[protocol.EventNotificationsDismissedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Notifications = slices.DeleteFunc(snapshot.Notifications, func(item protocol.NotificationNotice) bool { return slices.Contains(change.IDs, item.ID) })
	case "power_settings_changed":
		change, err := payload[protocol.EventPowerSettingsChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.PowerSettingsRevision = change.Revision
	case "terminal_settings_changed":
		change, err := payload[protocol.EventTerminalSettingsChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.TerminalSettingsRevision = change.Revision
	case "sandbox_settings_changed":
		change, err := payload[protocol.EventSandboxSettingsChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.SandboxSettingsRevision = change.Revision
	case "search_settings_changed":
		change, err := payload[protocol.EventSearchSettingsChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.SearchSettingsRevision = change.Revision
	case "compaction_settings_changed":
		change, err := payload[protocol.EventCompactionSettingsChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.CompactionSettingsRevision = change.Revision
	case "team_settings_changed":
		change, err := payload[protocol.EventTeamSettingsChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.TeamSettingsRevision = change.Revision
	case "media_settings_changed":
		settings, err := payload[protocol.MediaSettings](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.MediaSettings = settings
	case "terminal_changed":
		terminal, err := payload[protocol.TerminalInfo](event.Data)
		if err != nil {
			return false, err
		}
		found := false
		for index := range snapshot.Terminals {
			if snapshot.Terminals[index].ID == terminal.ID {
				snapshot.Terminals[index], found = terminal, true
				break
			}
		}
		if !found {
			snapshot.Terminals = append(snapshot.Terminals, terminal)
			slices.SortFunc(snapshot.Terminals, func(a, b protocol.TerminalInfo) int { return bytes.Compare(a.ID[:], b.ID[:]) })
		}
	case "plugin_changed":
		plugin, err := payload[protocol.PluginSummary](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Plugins = slices.DeleteFunc(snapshot.Plugins, func(item protocol.PluginSummary) bool { return item.Name == plugin.Name })
		snapshot.Plugins = append(snapshot.Plugins, plugin)
		slices.SortFunc(snapshot.Plugins, func(a, b protocol.PluginSummary) int { return cmp.Compare(a.Name, b.Name) })
	case "plugin_removed":
		change, err := payload[protocol.EventPluginRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Plugins = slices.DeleteFunc(snapshot.Plugins, func(item protocol.PluginSummary) bool { return item.Name == change.Name })
	case "role_changed":
		role, err := payload[protocol.RoleProfile](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Roles = slices.DeleteFunc(snapshot.Roles, func(item protocol.RoleProfile) bool { return item.ID == role.ID })
		snapshot.Roles = append(snapshot.Roles, role)
		slices.SortFunc(snapshot.Roles, func(a, b protocol.RoleProfile) int { return cmp.Compare(a.Key, b.Key) })
	case "role_removed":
		change, err := payload[protocol.EventRoleRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Roles = slices.DeleteFunc(snapshot.Roles, func(item protocol.RoleProfile) bool { return item.ID == change.ID })
		for _, team := range change.Teams {
			replaceTeam(snapshot, team)
		}
	case "team_changed":
		team, err := payload[protocol.TeamProfile](event.Data)
		if err != nil {
			return false, err
		}
		replaceTeam(snapshot, team)
	case "team_removed":
		change, err := payload[protocol.EventTeamRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Teams = slices.DeleteFunc(snapshot.Teams, func(item protocol.TeamProfile) bool { return item.ID == change.ID })
	case "ssh_changed":
		profile, err := payload[protocol.SshProfile](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Ssh = slices.DeleteFunc(snapshot.Ssh, func(item protocol.SshProfile) bool { return item.ID == profile.ID })
		snapshot.Ssh = append(snapshot.Ssh, profile)
		slices.SortFunc(snapshot.Ssh, func(a, b protocol.SshProfile) int { return bytes.Compare(a.ID[:], b.ID[:]) })
	case "ssh_removed":
		change, err := payload[protocol.EventSshRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Ssh = slices.DeleteFunc(snapshot.Ssh, func(item protocol.SshProfile) bool { return item.ID == change.ID })
	case "database_changed":
		profile, err := payload[protocol.DatabaseProfile](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Databases = slices.DeleteFunc(snapshot.Databases, func(item protocol.DatabaseProfile) bool { return item.ID == profile.ID })
		snapshot.Databases = append(snapshot.Databases, profile)
		slices.SortFunc(snapshot.Databases, func(a, b protocol.DatabaseProfile) int { return bytes.Compare(a.ID[:], b.ID[:]) })
	case "database_removed":
		change, err := payload[protocol.EventDatabaseRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Databases = slices.DeleteFunc(snapshot.Databases, func(item protocol.DatabaseProfile) bool { return item.ID == change.ID })
	case "provider_changed":
		provider, err := payload[protocol.ConversationProvider](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Providers = slices.DeleteFunc(snapshot.Providers, func(item protocol.ConversationProvider) bool { return item.ID == provider.ID })
		snapshot.Providers = append(snapshot.Providers, provider)
		retainQuotas(snapshot)
	case "provider_removed":
		change, err := payload[protocol.EventProviderRemovedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.Providers = slices.DeleteFunc(snapshot.Providers, func(item protocol.ConversationProvider) bool { return item.ID == change.ID })
		snapshot.ProviderQuotas = slices.DeleteFunc(snapshot.ProviderQuotas, func(item protocol.ConversationQuotaSnapshot) bool { return item.Provider == change.ID })
	case "provider_quota_changed":
		change, err := payload[protocol.EventProviderQuotaChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.ProviderQuotas = slices.DeleteFunc(snapshot.ProviderQuotas, func(item protocol.ConversationQuotaSnapshot) bool { return item.Provider == change.Provider })
		if change.Quota != nil && change.Quota.Provider == change.Provider && selectedQuota(snapshot, *change.Quota) {
			snapshot.ProviderQuotas = append(snapshot.ProviderQuotas, *change.Quota)
			slices.SortFunc(snapshot.ProviderQuotas, func(a, b protocol.ConversationQuotaSnapshot) int { return bytes.Compare(a.Provider[:], b.Provider[:]) })
		}
	case "model_catalog_changed":
		status, err := payload[protocol.ConversationCatalogStatus](event.Data)
		if err != nil {
			return false, err
		}
		snapshot.ModelCatalog = status
	case "credential_changed":
		change, err := payload[protocol.EventCredentialChangedPayload](event.Data)
		if err != nil {
			return false, err
		}
		if change.Credential.Node != projection.node {
			return false, fault(protocol.ErrorCodeWrongTarget, "credential belongs to another Node")
		}
	case "local_models_changed", "local_model_settings_changed", "knowledge_collections_changed",
		"telemetry_settings_changed", "backup_settings_changed", "python_settings_changed",
		"plugin_values_changed", "dispatch_changed", "retained_record", "tools_completed":
		// These invalidate feature reads; they do not replace a Snapshot field.
	default:
		return false, fault(protocol.ErrorCodeInvalidRequest, "unknown Node event")
	}
	return false, nil
}

func updateSession(snapshot *protocol.Snapshot, session protocol.Session) {
	for index := range snapshot.Sessions {
		if snapshot.Sessions[index].ID == session.ID {
			snapshot.Sessions[index] = session
			return
		}
	}
	snapshot.Sessions = append([]protocol.Session{session}, snapshot.Sessions...)
}

func updateTurn(snapshot *protocol.Snapshot, turn protocol.QueuedTurn) {
	snapshot.Turns = slices.DeleteFunc(snapshot.Turns, func(item protocol.TurnSummary) bool { return item.ID == turn.ID })
	snapshot.Turns = append(snapshot.Turns, protocol.TurnSummary{ID: turn.ID, Session: turn.Session, Request: turn.Request})
}

func replaceTeam(snapshot *protocol.Snapshot, team protocol.TeamProfile) {
	snapshot.Teams = slices.DeleteFunc(snapshot.Teams, func(item protocol.TeamProfile) bool { return item.ID == team.ID })
	snapshot.Teams = append(snapshot.Teams, team)
	slices.SortFunc(snapshot.Teams, func(a, b protocol.TeamProfile) int { return cmp.Compare(a.Key, b.Key) })
}

func selectedQuota(snapshot *protocol.Snapshot, quota protocol.ConversationQuotaSnapshot) bool {
	return quota.Credential.Node == snapshot.Node && slices.ContainsFunc(snapshot.Providers, func(provider protocol.ConversationProvider) bool {
		return provider.ID == quota.Provider && provider.Credential != nil && *provider.Credential == quota.Credential
	})
}

func retainQuotas(snapshot *protocol.Snapshot) {
	snapshot.ProviderQuotas = slices.DeleteFunc(snapshot.ProviderQuotas, func(quota protocol.ConversationQuotaSnapshot) bool { return !selectedQuota(snapshot, quota) })
}
