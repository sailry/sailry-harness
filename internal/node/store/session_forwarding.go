package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"reflect"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// transferable reports whether an authentication kind may leave the Node that
// holds it. Account sign-ins stay with the Node that performed them.
func transferable(authentication protocol.Authentication) error {
	switch authentication {
	case protocol.AuthenticationApiKey, protocol.AuthenticationCopilot:
		return nil
	case protocol.AuthenticationClaudeOAuth:
		return fault(protocol.ErrorCodeNotConfigured, "Claude sign-in is required on the execution Node")
	default:
		return fault(protocol.ErrorCodeNotConfigured, "ChatGPT sign-in is required on the execution Node")
	}
}

// exportCredential reads one transferable secret and its expiry for a paired Node.
func exportCredential(ctx context.Context, db *sql.Conn, node protocol.NodeID, reference protocol.CredentialRef, provider protocol.ConversationProvider) (protocol.Secret, *uint64, error) {
	if err := checkProviderWrite(ctx, db, provider.ID); err != nil {
		return protocol.Secret{}, nil, err
	}
	if err := transferable(provider.Authentication); err != nil {
		return protocol.Secret{}, nil, err
	}
	secret, err := resolveProviderKey(ctx, db, node, reference, provider.ID)
	if err != nil {
		return protocol.Secret{}, nil, err
	}
	saved, err := readProviderConfiguration(ctx, db, provider.ID)
	if err != nil {
		return protocol.Secret{}, nil, err
	}
	if saved == nil {
		return protocol.Secret{}, nil, fault(protocol.ErrorCodeNotFound, "source credential does not exist")
	}
	return *secret, saved.Authorizations[reference.ID].ExpiresAtMs, nil
}

// PrepareSessionForward freezes only controller configuration references. Runtime
// credentials reach the executor through its isolated cache, never this request.
// Replays retain the exact original metadata even after source edits or removal.
func PrepareSessionForward(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Request, error) {
	input, ok := request.Command.Data.(*protocol.CommandCreateSessionAtPayload)
	if !ok || input.Config == nil {
		return protocol.Request{}, invalid("configuration forwarding expected")
	}
	if caller != node || input.Target == node {
		return protocol.Request{}, fault(protocol.ErrorCodePermissionDenied, "only the local controller can forward configuration to another Node")
	}
	var frozen []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM session_forwards WHERE caller=?1 AND request=?2", caller[:], request.ID.String()).Scan(&frozen)
	if err == nil {
		var forwarded protocol.Request
		err = json.Unmarshal(frozen, &forwarded)
		return forwarded, err
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return protocol.Request{}, err
	}
	saved, err := readProviderConfiguration(ctx, db, input.Config.Provider)
	if err != nil {
		return protocol.Request{}, err
	}
	if saved == nil || !saved.Listed || saved.Provider == nil {
		return protocol.Request{}, fault(protocol.ErrorCodeNotFound, "source provider does not exist")
	}
	if saved.Provider.Revision != input.ProviderRevision {
		return protocol.Request{}, fault(protocol.ErrorCodeRevisionConflict, "source provider revision changed")
	}
	if err := validateConfig(ctx, db, node, *input.Config); err != nil {
		return protocol.Request{}, err
	}
	bundle := protocol.DefaultSessionImport()
	bundle.Source, bundle.Project, bundle.Worktree, bundle.Config = node, input.Project, input.Worktree, *input.Config
	bundle.Provider, err = CaptureProvider(ctx, db, *saved.Provider)
	if err != nil {
		return protocol.Request{}, err
	}
	// The explicit selection remains authoritative, including no credential.
	bundle.Provider.Credential = input.Config.Credential
	bundle.Roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, nil, nil)
	if err != nil {
		return protocol.Request{}, err
	}
	forwarded := protocol.Request{Version: request.Version, ID: request.ID, Target: input.Target, Command: protocol.Command{Kind: "import_session", Data: &bundle}}
	frozen, err = encode(forwarded)
	if err != nil {
		return protocol.Request{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO session_forwards(caller,request,body) VALUES(?1,?2,?3)", caller[:], request.ID.String(), frozen)
	return forwarded, err
}

// importedAuthorization validates one forwarded secret as a new credential.
func importedAuthorization(authentication protocol.Authentication, secret protocol.Secret, expiry *uint64) (providerAuthorization, error) {
	if err := transferable(authentication); err != nil {
		return providerAuthorization{}, err
	}
	if err := validateProviderKey(secret); err != nil {
		return providerAuthorization{}, err
	}
	if expiry != nil && (*expiry > math.MaxInt64 || *expiry <= uint64(time.Now().UnixMilli())) {
		return providerAuthorization{}, invalid("credential expiry must be in the future")
	}
	if authentication != protocol.AuthenticationApiKey {
		if _, err := providers.DecodeAccount(&secret, authentication); err != nil {
			return providerAuthorization{}, err
		}
	}
	return providerAuthorization{Authentication: authentication, Revision: 1, Key: &secret, ExpiresAtMs: expiry}, nil
}

// cachedForwardProvider resolves the retained controller revision and its current
// runtime credential. A forward cannot materialize configuration or bypass a
// missing cache by attaching a secret to the session request.
func cachedForwardProvider(ctx context.Context, db *sql.Conn, node protocol.NodeID, source protocol.ConversationProvider) (protocol.ConversationProvider, error) {
	record, err := readConfigurationCacheRecord(ctx, db, "providers", source.ID.String(), source.Revision)
	if err != nil {
		return protocol.ConversationProvider{}, err
	}
	if record == nil {
		return protocol.ConversationProvider{}, fault(protocol.ErrorCodeNotConfigured, "captured controller provider is unavailable on the execution Node")
	}
	var provider protocol.ConversationProvider
	if err := json.Unmarshal(record.Body, &provider); err != nil {
		return provider, err
	}
	provider, err = CaptureProvider(ctx, db, provider)
	if err != nil {
		return provider, err
	}
	// Owner/projection metadata differs across Nodes. The explicit credential
	// is selected independently of the provider's current default account.
	expected := source
	expected.Owner, expected.Shared, expected.Source, expected.Consent = nil, false, nil, nil
	provider.Owner, provider.Shared, provider.Source, provider.Consent = nil, false, nil, nil
	provider.Credential = source.Credential
	if !reflect.DeepEqual(provider, expected) {
		return provider, fault(protocol.ErrorCodeRevisionConflict, "captured controller provider differs from its cached revision")
	}
	if provider.Credential != nil {
		if _, err := ReadRuntimeProviderCredential(ctx, db, node, provider, *provider.Credential); err != nil {
			// Expiry is recoverable at the next unsent model step. Identity,
			// revocation and missing-cache faults still reject the capture.
			if provider.Authentication == protocol.AuthenticationApiKey || !expiredRuntimeCredential(err) {
				return provider, err
			}
		}
	}
	return provider, nil
}

func cachedForwardRoles(ctx context.Context, db *sql.Conn, node protocol.NodeID, snapshot protocol.RoleSnapshot) (protocol.RoleSnapshot, error) {
	if snapshot.Execution != protocol.ConversationExecutionModel && snapshot.Execution != protocol.ConversationExecutionTeam {
		return snapshot, invalid("invalid execution selection")
	}
	if snapshot.Execution == protocol.ConversationExecutionModel && (len(snapshot.Profiles) != 0 || snapshot.Team != nil || len(snapshot.Instructions) != 0 || len(snapshot.Models) != 0 || len(snapshot.Providers) != 0) {
		return snapshot, invalid("model execution cannot include a delegation roster")
	}
	for _, role := range snapshot.Profiles {
		record, err := readConfigurationCacheRecord(ctx, db, "roles", role.ID.String(), role.Revision)
		if err != nil {
			return snapshot, err
		}
		if record == nil {
			return snapshot, fault(protocol.ErrorCodeNotConfigured, "captured controller role is unavailable on the execution Node")
		}
		var cached protocol.RoleProfile
		if err := json.Unmarshal(record.Body, &cached); err != nil {
			return snapshot, err
		}
		role.Source, cached.Source = nil, nil
		if !reflect.DeepEqual(role, cached) {
			return snapshot, fault(protocol.ErrorCodeRevisionConflict, "captured controller role differs from its cached revision")
		}
	}
	if snapshot.Team != nil {
		record, err := readConfigurationCacheRecord(ctx, db, "teams", snapshot.Team.ID.String(), snapshot.Team.Revision)
		if err != nil {
			return snapshot, err
		}
		if record == nil {
			return snapshot, fault(protocol.ErrorCodeNotConfigured, "captured controller team is unavailable on the execution Node")
		}
		var team protocol.TeamProfile
		if err := json.Unmarshal(record.Body, &team); err != nil {
			return snapshot, err
		}
		if team.Instructions != snapshot.Instructions {
			return snapshot, invalid("session team instructions differ from their capture")
		}
	}
	providers := make(map[protocol.ProviderID]protocol.ConversationProvider, len(snapshot.Providers))
	for _, source := range snapshot.Providers {
		if _, exists := providers[source.ID]; exists {
			return snapshot, invalid("session role providers must be distinct")
		}
		provider, err := cachedForwardProvider(ctx, db, node, source)
		if err != nil {
			return snapshot, err
		}
		providers[source.ID] = provider
	}
	// completeRoleSnapshot validates role/model selections. Resolve exclusively
	// against the frozen providers so a newer default cannot replace a capture.
	return completeCapturedRoleSnapshot(snapshot, providers)
}

func completeCapturedRoleSnapshot(snapshot protocol.RoleSnapshot, providers map[protocol.ProviderID]protocol.ConversationProvider) (protocol.RoleSnapshot, error) {
	ids, keys, used := map[protocol.RoleID]bool{}, map[string]bool{}, map[protocol.ProviderID]bool{}
	validate := func(model protocol.RoleModel) error {
		provider, ok := providers[model.Provider]
		if !ok {
			return invalid("session role providers do not match the selected models")
		}
		used[provider.ID] = true
		return validateRoleModel(provider, model)
	}
	for _, role := range snapshot.Profiles {
		if err := ValidateRole(role); err != nil {
			return snapshot, err
		}
		if ids[role.ID] || keys[role.Key] {
			return snapshot, invalid("session roles must have distinct identities and keys")
		}
		ids[role.ID], keys[role.Key] = true, true
		if role.Model != nil {
			if err := validate(*role.Model); err != nil {
				return snapshot, err
			}
		}
	}
	for id, model := range snapshot.Models {
		if !ids[id] {
			return snapshot, invalid("member model requires a role in the session roster")
		}
		if err := validate(model); err != nil {
			return snapshot, err
		}
	}
	if len(used) != len(providers) {
		return snapshot, invalid("session role providers do not match the selected models")
	}
	for index, source := range snapshot.Providers {
		snapshot.Providers[index] = providers[source.ID]
	}
	return snapshot, nil
}

// importSession retains canonical controller captures. Old private provider rows
// remain readable for historical sessions, but new imports never create them.
func importSession(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, profile string, bundle protocol.SessionImport) (protocol.Output, []protocol.Event, error) {
	if bundle.Source != caller || caller == node {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "configuration source does not match the paired caller")
	}
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if !cached || user.Controller == nil || *user.Controller != caller {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "session configuration does not belong to this controller")
	}
	selected, captured := bundle.Config.Credential, bundle.Provider.Credential
	if bundle.Config.Provider != bundle.Provider.ID || (selected == nil) != (captured == nil) || selected != nil && (*selected != *captured || selected.Node != caller) ||
		bundle.Secret != nil || bundle.ExpiresAtMs != nil || len(bundle.RoleCredentials) != 0 {
		return protocol.Output{}, nil, invalid("session imports use cached controller credentials")
	}
	provider, err := cachedForwardProvider(ctx, db, node, bundle.Provider)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	roles, err := cachedForwardRoles(ctx, db, node, bundle.Roles)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return createProfiledSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Project: bundle.Project, Worktree: bundle.Worktree, Config: &bundle.Config}, &protocol.SessionProfile{Source: caller, Provider: provider}, &roles)
}

// removePrivateProviders deletes the providers forwarded for removed sessions
// once no remaining session uses them. Their secrets leave with them.
func removePrivateProviders(ctx context.Context, db *sql.Conn, candidates []protocol.ProviderID) error {
	for _, id := range candidates {
		stored, err := readProviderConfiguration(ctx, db, id)
		if err != nil {
			return err
		}
		if stored == nil || stored.Listed {
			continue
		}
		var used bool
		err = db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision
 WHERE s.state!='removed' AND (json_extract(r.config,'$.provider')=?1 OR json_extract(r.profile,'$.provider.id')=?1
 OR EXISTS(SELECT 1 FROM json_each(r.roles,'$.providers') p WHERE json_extract(p.value,'$.id')=?1)))`, id.String()).Scan(&used)
		if err != nil {
			return err
		}
		if used {
			continue
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM providers WHERE id=?1", id.String()); err != nil {
			return err
		}
	}
	return nil
}

// ListAssociatedSessions pages the sessions a package associated with one
// business identity, newest first. A worktree scope confines the page.
func ListAssociatedSessions(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.ConversationAssociationAssociatedSessions, error) {
	page := protocol.DefaultConversationAssociationAssociatedSessions()
	input := request.Command.Data.(*protocol.CommandListAssociatedSessionsPayload)
	if err := validateSessionAssociation(input.Association); err != nil {
		return page, err
	}
	if input.Limit < 1 || input.Limit > 100 {
		return page, invalid("session page limit must be between 1 and 100")
	}
	var worktree *protocol.WorktreeID
	if request.Plugin != nil {
		worktree = request.Plugin.Worktree
	}
	var scope any
	if worktree != nil {
		scope = worktree.String()
	}
	sequence := uint64(math.MaxInt64)
	if input.Before != nil {
		if err := CheckSessionAccess(ctx, db, *input.Before); err != nil {
			return page, err
		}
		session, err := ReadSession(ctx, db, *input.Before)
		if err != nil {
			return page, err
		}
		if session.Association == nil || *session.Association != input.Association || worktree != nil && *worktree != session.Worktree {
			return page, fault(protocol.ErrorCodeWrongTarget, "session cursor belongs to another association")
		}
		sequence = session.CreationOrder
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return page, err
	}
	rows, err := db.QueryContext(ctx, `SELECT s.id FROM sessions s JOIN session_associations a ON a.session=s.id LEFT JOIN projects p ON p.id=s.project
 WHERE a.namespace=?1 AND a.kind=?2 AND a.identity=?3 AND s.state!='removed'
 AND s.rowid<?4 AND (?5 IS NULL OR s.worktree=?5) AND (CASE WHEN s.project IS NULL THEN coalesce(s.owner,(SELECT id FROM users WHERE local=1))=?7 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?7 END) ORDER BY s.rowid DESC LIMIT ?6`,
		input.Association.Namespace, input.Association.Kind, input.Association.ID, int64(sequence), scope, int64(input.Limit)+1, user.ID.String())
	if err != nil {
		return page, err
	}
	ids := []protocol.SessionID{}
	for rows.Next() {
		var text string
		if err := rows.Scan(&text); err != nil {
			rows.Close()
			return page, err
		}
		id, err := protocol.ParseSessionID(text)
		if err != nil {
			rows.Close()
			return page, err
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return page, err
	}
	if err := rows.Close(); err != nil {
		return page, err
	}
	if len(ids) > int(input.Limit) {
		ids = ids[:input.Limit]
		next := ids[len(ids)-1]
		page.NextBefore = &next
	}
	for _, id := range ids {
		session, err := ReadSession(ctx, db, id)
		if err != nil {
			return page, err
		}
		page.Sessions = append(page.Sessions, session)
	}
	return page, nil
}
