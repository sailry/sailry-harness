package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"math"
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
	if err := transferable(provider.Authentication); err != nil {
		return protocol.Secret{}, nil, err
	}
	secret, err := resolveProviderKey(ctx, db, node, reference, provider.ID)
	if err != nil {
		return protocol.Secret{}, nil, err
	}
	saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", provider.ID.String())
	if err != nil {
		return protocol.Secret{}, nil, err
	}
	if saved == nil {
		return protocol.Secret{}, nil, fault(protocol.ErrorCodeNotFound, "source credential does not exist")
	}
	return *secret, saved.Authorizations[reference.ID].ExpiresAtMs, nil
}

// PrepareSessionForward packages the local provider, its transferable secret
// and the captured roles into the import a paired Node admits under the same
// request identifier. Only the local controller may forward, and only to
// another Node.
func PrepareSessionForward(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Request, error) {
	input, ok := request.Command.Data.(*protocol.CommandCreateSessionAtPayload)
	if !ok || input.Config == nil {
		return protocol.Request{}, invalid("configuration forwarding expected")
	}
	if caller != node || input.Target == node {
		return protocol.Request{}, fault(protocol.ErrorCodePermissionDenied, "only the local controller can forward configuration to another Node")
	}
	saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", input.Config.Provider.String())
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
	bundle.Provider = *saved.Provider
	// The explicit selection is authoritative, including an absent credential.
	bundle.Provider.Credential = input.Config.Credential
	if reference := input.Config.Credential; reference != nil {
		secret, expiry, err := exportCredential(ctx, db, node, *reference, bundle.Provider)
		if err != nil {
			return protocol.Request{}, err
		}
		bundle.Secret, bundle.ExpiresAtMs = &secret, expiry
	}
	bundle.Roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, nil, nil)
	if err != nil {
		return protocol.Request{}, err
	}
	bundle.RoleCredentials = []protocol.RoleProviderSecret{}
	for _, provider := range bundle.Roles.Providers {
		if provider.Credential == nil {
			continue
		}
		secret, expiry, err := exportCredential(ctx, db, node, *provider.Credential, provider)
		if err != nil {
			return protocol.Request{}, err
		}
		bundle.RoleCredentials = append(bundle.RoleCredentials, protocol.RoleProviderSecret{Provider: provider.ID, Secret: secret, ExpiresAtMs: expiry})
	}
	return protocol.Request{Version: request.Version, ID: request.ID, Target: input.Target, Command: protocol.Command{Kind: "import_session", Data: &bundle}}, nil
}

// importedAuthorization validates one forwarded secret as a new credential.
func importedAuthorization(ctx context.Context, db *sql.Conn, authentication protocol.Authentication, secret protocol.Secret, expiry *uint64) (providerAuthorization, error) {
	if err := transferable(authentication); err != nil {
		return providerAuthorization{}, err
	}
	if err := validateProviderKey(secret); err != nil {
		return providerAuthorization{}, err
	}
	if expiry != nil && (*expiry > math.MaxInt64 || *expiry <= uint64(time.Now().UnixMilli())) {
		return providerAuthorization{}, invalid("credential expiry must be in the future")
	}
	if err := providerCredentialCapacity(ctx, db); err != nil {
		return providerAuthorization{}, err
	}
	if authentication != protocol.AuthenticationApiKey {
		if _, err := providers.DecodeAccount(&secret, authentication); err != nil {
			return providerAuthorization{}, err
		}
	}
	return providerAuthorization{Authentication: authentication, Revision: 1, Key: &secret, ExpiresAtMs: expiry}, nil
}

// retainPrivateProvider stores a forwarded provider under a new identity that
// cannot alias a provider configured on this Node. It is never listed.
func retainPrivateProvider(ctx context.Context, db *sql.Conn, node protocol.NodeID, source protocol.ConversationProvider, secret *protocol.Secret, expiry *uint64) (protocol.ConversationProvider, error) {
	var provider protocol.ConversationProvider
	raw, err := encode(source)
	if err != nil {
		return provider, err
	}
	if err := json.Unmarshal(raw, &provider); err != nil {
		return provider, err
	}
	provider.ID, provider.Credential = protocol.NewProviderID(), nil
	stored := providerConfiguration{ID: provider.ID, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
	if secret != nil {
		authorization, err := importedAuthorization(ctx, db, provider.Authentication, *secret, expiry)
		if err != nil {
			return provider, err
		}
		id := protocol.NewCredentialID()
		stored.Authorizations[id] = authorization
		provider.Credential = &protocol.CredentialRef{Node: node, ID: id}
	}
	if err := providers.ValidateConfiguration(provider); err != nil {
		return provider, err
	}
	configured := provider
	stored.Provider = &configured
	return provider, saveProviderConfiguration(ctx, db, stored)
}

// importSessionRoles stores each forwarded role provider privately and rewrites
// the snapshot to those identities.
func importSessionRoles(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, snapshot protocol.RoleSnapshot, credentials []protocol.RoleProviderSecret) (protocol.RoleSnapshot, error) {
	selected := map[protocol.ProviderID]bool{}
	for _, role := range snapshot.Profiles {
		if role.Model != nil {
			selected[role.Model.Provider] = true
		}
	}
	for _, model := range snapshot.Models {
		selected[model.Provider] = true
	}
	required := map[protocol.ProviderID]bool{}
	for _, provider := range snapshot.Providers {
		if !selected[provider.ID] {
			return snapshot, invalid("session role providers do not match the selected models")
		}
		delete(selected, provider.ID)
		if provider.Credential != nil {
			if provider.Credential.Node != caller {
				return snapshot, invalid("role credential import is inconsistent")
			}
			required[provider.ID] = true
		}
	}
	if len(selected) != 0 {
		return snapshot, invalid("session role providers do not match the selected models")
	}
	supplied := map[protocol.ProviderID]protocol.RoleProviderSecret{}
	for _, credential := range credentials {
		if _, duplicate := supplied[credential.Provider]; duplicate || !required[credential.Provider] {
			return snapshot, invalid("role credential import is inconsistent")
		}
		supplied[credential.Provider] = credential
	}
	if len(supplied) != len(required) {
		return snapshot, invalid("role credential import is inconsistent")
	}
	profiles := make([]protocol.RoleProfile, len(snapshot.Profiles))
	copy(profiles, snapshot.Profiles)
	models := make(map[protocol.RoleID]protocol.RoleModel, len(snapshot.Models))
	for id, model := range snapshot.Models {
		models[id] = model
	}
	imported := make([]protocol.ConversationProvider, 0, len(snapshot.Providers))
	for _, source := range snapshot.Providers {
		captured, err := CaptureProvider(ctx, db, source)
		if err != nil {
			return snapshot, err
		}
		// A partner channel keeps its policy; any other captured binding is
		// resolved again on this Node.
		if captured.Preset == nil || captured.Preset.Source.Kind != "partner" {
			captured.Binding = nil
		} else if err := providers.ValidateBinding(captured); err != nil {
			return snapshot, err
		}
		var secret *protocol.Secret
		var expiry *uint64
		if credential, exists := supplied[source.ID]; exists {
			secret, expiry = &credential.Secret, credential.ExpiresAtMs
		}
		provider, err := retainPrivateProvider(ctx, db, node, captured, secret, expiry)
		if err != nil {
			return snapshot, err
		}
		for index := range profiles {
			if model := profiles[index].Model; model != nil && model.Provider == source.ID {
				rewritten := *model
				rewritten.Provider = provider.ID
				profiles[index].Model = &rewritten
			}
		}
		for id, model := range models {
			if model.Provider == source.ID {
				model.Provider = provider.ID
				models[id] = model
			}
		}
		imported = append(imported, provider)
	}
	snapshot.Profiles, snapshot.Models, snapshot.Providers = profiles, models, imported
	if snapshot.Execution == protocol.ConversationExecutionModel {
		if len(snapshot.Profiles) != 0 || snapshot.Team != nil || len(snapshot.Instructions) != 0 || len(snapshot.Models) != 0 {
			return snapshot, invalid("model execution cannot include a delegation roster")
		}
		return snapshot, nil
	}
	return completeRoleSnapshot(ctx, db, node, snapshot)
}

// importSession creates a session from the configuration a paired Node
// forwarded. The provider and its secret are private to that session.
func importSession(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, profile string, bundle protocol.SessionImport) (protocol.Output, []protocol.Event, error) {
	if bundle.Source != caller || caller == node {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "configuration source does not match the paired caller")
	}
	selected, captured := bundle.Config.Credential, bundle.Provider.Credential
	if bundle.Config.Provider != bundle.Provider.ID || (selected == nil) != (captured == nil) || selected != nil && (*selected != *captured || selected.Node != caller) ||
		(selected == nil) != (bundle.Secret == nil) || bundle.Secret == nil && bundle.ExpiresAtMs != nil {
		return protocol.Output{}, nil, invalid("configuration import is inconsistent")
	}
	provider, err := retainPrivateProvider(ctx, db, node, bundle.Provider, bundle.Secret, bundle.ExpiresAtMs)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	config := bundle.Config
	config.Provider, config.Credential = provider.ID, provider.Credential
	roles, err := importSessionRoles(ctx, db, node, caller, bundle.Roles, bundle.RoleCredentials)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if config.Assistant != nil {
		installed, err := ReadPlugin(ctx, db, config.Assistant.Package.Name)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if installed == nil || installed.Summary.Compatibility.Kind != "compatible" || installed.Summary.Digest != config.Assistant.Package.Digest {
			return protocol.Output{}, nil, invalid("plugin assistant package is unavailable on the destination Node")
		}
		binding := *config.Assistant
		binding.Package = protocol.PluginReference{Name: installed.Summary.Name, Digest: installed.Summary.Digest, SettingsRevision: installed.Summary.SettingsRevision}
		config.Assistant = &binding
	}
	provider, err = CaptureProvider(ctx, db, provider)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return createProfiledSession(ctx, db, node, profile, protocol.CommandCreateSessionPayload{Project: bundle.Project, Worktree: bundle.Worktree, Config: &config}, &protocol.SessionProfile{Source: caller, Provider: provider}, &roles)
}

// removePrivateProviders deletes the providers forwarded for removed sessions
// once no remaining session uses them. Their secrets leave with them.
func removePrivateProviders(ctx context.Context, db *sql.Conn, candidates []protocol.ProviderID) error {
	for _, id := range candidates {
		stored, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", id.String())
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
		session, err := ReadSession(ctx, db, *input.Before)
		if err != nil {
			return page, err
		}
		if session.Association == nil || *session.Association != input.Association || worktree != nil && *worktree != session.Worktree {
			return page, fault(protocol.ErrorCodeWrongTarget, "session cursor belongs to another association")
		}
		sequence = session.CreationOrder
	}
	rows, err := db.QueryContext(ctx, `SELECT s.id FROM sessions s JOIN session_associations a ON a.session=s.id
 WHERE a.namespace=?1 AND a.kind=?2 AND a.identity=?3 AND s.state!='removed'
 AND s.rowid<?4 AND (?5 IS NULL OR s.worktree=?5) ORDER BY s.rowid DESC LIMIT ?6`,
		input.Association.Namespace, input.Association.Kind, input.Association.ID, int64(sequence), scope, int64(input.Limit)+1)
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
