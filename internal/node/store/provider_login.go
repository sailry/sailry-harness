package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ProviderLoginCapture struct {
	Attempt  protocol.ConversationLoginAttempt
	Provider protocol.ConversationProvider
	Target   providers.AccountTarget
}

func pluginAccountOrigin(info protocol.PluginInfo) protocol.ConversationConsentOrigin {
	if info.Origin != nil {
		switch info.Origin.Kind {
		case "bundled":
			return protocol.ConversationConsentOrigin{Kind: "bundled"}
		case "online":
			var origin protocol.PluginOriginOnlinePayload
			body, _ := json.Marshal(info.Origin.Data)
			if json.Unmarshal(body, &origin) == nil {
				return protocol.ConversationConsentOrigin{Kind: "git", Data: protocol.ConversationConsentOriginGitPayload{Repository: origin.Source.Repository, Path: origin.Path}}
			}
		case "registry":
			var origin protocol.PluginOriginRegistryPayload
			body, _ := json.Marshal(info.Origin.Data)
			if json.Unmarshal(body, &origin) == nil {
				return protocol.ConversationConsentOrigin{Kind: "registry", Data: protocol.ConversationConsentOriginRegistryPayload{Namespace: origin.Source.Namespace, Slug: origin.Source.Slug}}
			}
		case "worktree":
			var origin protocol.PluginOriginWorktreePayload
			body, _ := json.Marshal(info.Origin.Data)
			if json.Unmarshal(body, &origin) == nil {
				return protocol.ConversationConsentOrigin{Kind: "worktree", Data: protocol.ConversationConsentOriginWorktreePayload{Worktree: origin.Worktree, Path: origin.Path}}
			}
		}
	}
	return protocol.ConversationConsentOrigin{Kind: "imported", Data: protocol.ConversationConsentOriginImportedPayload{Digest: info.Summary.Digest}}
}

func providerAuthDeclaration(ctx context.Context, db *sql.Conn, provider protocol.ConversationProvider) (protocol.PluginInfo, protocol.PluginModelAuthDeclaration, error) {
	var info protocol.PluginInfo
	var flow string
	if provider.Authentication == protocol.AuthenticationPlugin {
		if provider.Binding == nil || *provider.Binding == nil || (**provider.Binding).Package == nil || (**provider.Binding).Auth == nil {
			return info, protocol.PluginModelAuthDeclaration{}, fault(protocol.ErrorCodeNotConfigured, "channel authentication package is not captured")
		}
		binding := **provider.Binding
		current, err := ReadPlugin(ctx, db, binding.Package.Name)
		if err != nil {
			return info, protocol.PluginModelAuthDeclaration{}, err
		}
		if current == nil || !current.Summary.Enabled || !reflect.DeepEqual(plugins.PackageReference(*current), *binding.Package) {
			return info, protocol.PluginModelAuthDeclaration{}, fault(protocol.ErrorCodeNotConfigured, "channel authentication package is unavailable")
		}
		info, flow = *current, binding.Auth.Flow
	} else {
		infos, err := ReadPluginInfos(ctx, db)
		if err != nil {
			return info, protocol.PluginModelAuthDeclaration{}, err
		}
		for _, candidate := range infos {
			if !candidate.Summary.Enabled || candidate.Extension == nil || candidate.Summary.Compatibility.Kind != "compatible" {
				continue
			}
			for _, declaration := range candidate.Extension.ModelAuth {
				if declaration.Authentication != nil && *declaration.Authentication == provider.Authentication {
					if flow != "" {
						return info, declaration, invalid("multiple enabled packages provide the same account authentication")
					}
					info, flow = candidate, declaration.ID
				}
			}
		}
	}
	if info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionAuthenticateModels) {
		return info, protocol.PluginModelAuthDeclaration{}, fault(protocol.ErrorCodeNotConfigured, "model authentication is not declared by an enabled package")
	}
	for _, declaration := range info.Extension.ModelAuth {
		if declaration.ID != flow {
			continue
		}
		if (declaration.Flow.Kind == "script" || declaration.Exchange != nil) && !slices.Contains(info.Extension.Actions, protocol.PluginActionInspectCredentials) {
			return info, declaration, fault(protocol.ErrorCodePermissionDenied, "model authentication package cannot inspect credentials")
		}
		return info, declaration, nil
	}
	return info, protocol.PluginModelAuthDeclaration{}, fault(protocol.ErrorCodeNotConfigured, "model authentication flow is unavailable")
}

func CaptureProviderLogin(ctx context.Context, db *sql.Conn, id protocol.ProviderID, expected uint64, attempt protocol.RequestID) (ProviderLoginCapture, error) {
	var capture ProviderLoginCapture
	saved, err := currentProvider(ctx, db, id, &expected)
	if err != nil {
		return capture, err
	}
	if saved.Provider.Authentication == protocol.AuthenticationApiKey {
		return capture, invalid("API-key providers do not use account sign-in")
	}
	provider, err := CaptureProvider(ctx, db, *saved.Provider)
	if err != nil {
		return capture, err
	}
	return captureProviderLogin(ctx, db, provider, attempt)
}

func captureProviderLogin(ctx context.Context, db *sql.Conn, provider protocol.ConversationProvider, attempt protocol.RequestID) (ProviderLoginCapture, error) {
	var capture ProviderLoginCapture
	info, declaration, err := providerAuthDeclaration(ctx, db, provider)
	if err != nil {
		return capture, err
	}
	reference := plugins.PackageReference(info)
	resolved, err := ResolvePluginSettings(ctx, db, reference)
	if err != nil {
		return capture, err
	}
	fields, err := providers.OAuthFields(provider)
	if err != nil {
		return capture, err
	}
	if declaration.Client.ClientID != nil {
		fields["client_id"] = *declaration.Client.ClientID
	} else if name := declaration.Client.ClientIdSetting; name != nil {
		var clientID string
		if json.Unmarshal(resolved.Values[*name], &clientID) != nil {
			return capture, fault(protocol.ErrorCodeNotConfigured, "model authentication client ID is not configured")
		}
		fields["client_id"] = clientID
	}
	if name := declaration.Client.ClientSecretSetting; name != nil {
		value, exists := resolved.Secrets[*name]
		if !exists {
			return capture, fault(protocol.ErrorCodeNotConfigured, "model authentication client secret is not configured")
		}
		fields["client_secret"] = value.Expose()
	}
	declaration.Scopes = slices.Clone(declaration.Scopes)
	slices.Sort(declaration.Scopes)
	declaration.Scopes = slices.Compact(declaration.Scopes)
	capture = ProviderLoginCapture{
		Attempt: protocol.ConversationLoginAttempt{ID: attempt, Provider: provider.ID}, Provider: provider,
		Target: providers.AccountTarget{Provider: provider.ID, Package: reference, Origin: pluginAccountOrigin(info), Flow: declaration.ID, Endpoint: provider.Endpoint, Declaration: declaration, Fields: fields},
	}
	return capture, nil
}

func CompleteProviderLogin(ctx context.Context, db *sql.Conn, node protocol.NodeID, capture ProviderLoginCapture, grant providers.AccountGrant) ([]protocol.Event, error) {
	current, err := CaptureProviderLogin(ctx, db, capture.Provider.ID, capture.Provider.Revision, capture.Attempt.ID)
	if err != nil {
		return nil, err
	}
	if !reflect.DeepEqual(current.Provider, capture.Provider) || !reflect.DeepEqual(current.Target, capture.Target) {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "provider authentication configuration changed during sign-in")
	}
	saved, err := currentProvider(ctx, db, capture.Provider.ID, &capture.Provider.Revision)
	if err != nil {
		return nil, err
	}
	if err := providerCredentialCapacity(ctx, db); err != nil {
		return nil, err
	}
	if grant.Kind != saved.Provider.Authentication {
		return nil, invalid("authorization authentication does not match the provider")
	}
	body, err := json.Marshal(grant)
	if err != nil {
		return nil, err
	}
	if grant.Kind != protocol.AuthenticationPlugin {
		if err := authorization.ValidateGrant(body, grant.Kind, time.Now()); err != nil {
			return nil, err
		}
	} else if grant.Record == nil || !grant.Record.Target.Matches(capture.Target) {
		return nil, invalid("model authorization returned a different account owner")
	}
	secret := protocol.NewSecret(string(body))
	id := protocol.NewCredentialID()
	if saved.Authorizations == nil {
		saved.Authorizations = map[protocol.CredentialID]providerAuthorization{}
	}
	authorization := providerAuthorization{Authentication: grant.Kind, Revision: 1, Key: &secret, ExpiresAtMs: grant.Expiry()}
	if grant.Kind != protocol.AuthenticationPlugin && grant.Kind != protocol.AuthenticationCopilot {
		target := capture.Target
		authorization.Auth = &target
	}
	saved.Authorizations[id] = authorization
	saved.Provider.Credential = &protocol.CredentialRef{Node: node, ID: id}
	saved.Provider.Revision, err = nextRevision(saved.Provider.Revision)
	if err != nil {
		return nil, err
	}
	if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
		return nil, err
	}
	events := []protocol.Event{
		{Kind: "provider_changed", Data: *saved.Provider},
		{Kind: "credential_changed", Data: protocol.EventCredentialChangedPayload{Credential: *saved.Provider.Credential, Revision: 1}},
	}
	quotaChanges, err := ReconcileProviderQuotas(ctx, db, node)
	return append(events, quotaChanges...), err
}

func OAuthHost(ctx context.Context, db *sql.Conn) (string, error) {
	host := protocol.NewRequestID().String()
	if _, err := db.ExecContext(ctx, "INSERT INTO oauth_hosts(singleton,host_id) VALUES(1,?1) ON CONFLICT(singleton) DO NOTHING", host); err != nil {
		return "", err
	}
	err := db.QueryRowContext(ctx, "SELECT host_id FROM oauth_hosts WHERE singleton=1").Scan(&host)
	return host, err
}

func OAuthRegistration(ctx context.Context, db *sql.Conn, key string) (*string, error) {
	var value string
	err := db.QueryRowContext(ctx, "SELECT client_id FROM oauth_registrations WHERE provider=?1", key).Scan(&value)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &value, err
}

func SaveOAuthRegistration(ctx context.Context, db *sql.Conn, key, client string) error {
	_, err := db.ExecContext(ctx, "INSERT INTO oauth_registrations(provider,client_id) VALUES(?1,?2) ON CONFLICT(provider) DO UPDATE SET client_id=excluded.client_id", key, client)
	return err
}

type ProviderRenewal struct {
	Capture    ProviderLoginCapture
	Credential protocol.CredentialID
	Revision   uint64
	Grant      providers.AccountGrant
}

func CaptureProviderAuthorization(ctx context.Context, db *sql.Conn, id protocol.ProviderID) (protocol.ConversationProvider, error) {
	saved, err := currentProvider(ctx, db, id, nil)
	if err != nil {
		return protocol.ConversationProvider{}, err
	}
	return CaptureProvider(ctx, db, *saved.Provider)
}

func ReadProviderAuthorization(ctx context.Context, db *sql.Conn, node protocol.NodeID, provider protocol.ConversationProvider, reference protocol.CredentialRef) (*protocol.Secret, error) {
	saved, err := currentProvider(ctx, db, provider.ID, nil)
	if err != nil {
		return nil, err
	}
	value, exists := saved.Authorizations[reference.ID]
	if !exists || value.Authentication != provider.Authentication {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider authorization is unavailable")
	}
	grant, err := providers.DecodeAccount(value.Key, provider.Authentication)
	if err != nil {
		return nil, err
	}
	capture, err := captureProviderLogin(ctx, db, provider, protocol.NewRequestID())
	if err != nil {
		return nil, err
	}
	if grant.Kind == protocol.AuthenticationPlugin {
		if grant.Record == nil || !grant.Record.Target.Matches(capture.Target) {
			return nil, fault(protocol.ErrorCodeNotConfigured, "model authentication changed; sign in again")
		}
	} else if grant.Kind != protocol.AuthenticationCopilot && (value.Auth == nil || !value.Auth.Matches(capture.Target)) {
		return nil, fault(protocol.ErrorCodeNotConfigured, "model authentication changed; sign in again")
	}
	return resolveProviderKey(ctx, db, node, reference, provider.ID)
}

func BeginProviderRenewal(ctx context.Context, db *sql.Conn, node protocol.NodeID, provider protocol.ConversationProvider, reference protocol.CredentialRef, now time.Time) (*ProviderRenewal, error) {
	if reference.Node != node {
		return nil, fault(protocol.ErrorCodePermissionDenied, "credential belongs to another Node")
	}
	saved, err := currentProvider(ctx, db, provider.ID, nil)
	if err != nil || provider.Authentication == protocol.AuthenticationApiKey {
		return nil, err
	}
	if saved.Provider.Authentication != provider.Authentication {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider authentication changed")
	}
	id := reference.ID
	value, exists := saved.Authorizations[id]
	if !exists || value.Authentication != provider.Authentication {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider authorization is unavailable")
	}
	grant, err := providers.DecodeAccount(value.Key, provider.Authentication)
	if err != nil {
		return nil, err
	}
	capture, err := captureProviderLogin(ctx, db, provider, protocol.NewRequestID())
	if err != nil {
		return nil, err
	}
	if grant.Kind == protocol.AuthenticationPlugin {
		if grant.Record == nil || !grant.Record.Target.Matches(capture.Target) {
			return nil, fault(protocol.ErrorCodeNotConfigured, "model authentication changed; sign in again")
		}
	} else if grant.Kind != protocol.AuthenticationCopilot && (value.Auth == nil || !value.Auth.Matches(capture.Target)) {
		return nil, fault(protocol.ErrorCodeNotConfigured, "model authentication changed; sign in again")
	}
	expiry := grant.Expiry()
	if expiry == nil || *expiry > uint64(now.Add(time.Minute).UnixMilli()) {
		return nil, nil
	}
	if grant.EarliestRefreshAt != nil && *grant.EarliestRefreshAt > uint64(now.UnixMilli()) {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider renewal is not available yet")
	}
	if capture.Target.Declaration.Flow.Kind == "script" {
		var flow protocol.PluginModelAuthFlowScriptPayload
		body, _ := json.Marshal(capture.Target.Declaration.Flow.Data)
		if json.Unmarshal(body, &flow) != nil || !flow.Refresh {
			return nil, fault(protocol.ErrorCodeNotConfigured, "provider authorization expired; sign in again")
		}
	} else if capture.Target.Declaration.Refresh == nil || grant.Record == nil || grant.Record.Issued.Refresh == nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider authorization expired; sign in again")
	}
	previous := grant
	grant.RenewalPending = true
	body, err := json.Marshal(grant)
	if err != nil {
		return nil, err
	}
	secret := protocol.NewSecret(string(body))
	value.Key = &secret
	saved.Authorizations[id] = value
	if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
		return nil, err
	}
	return &ProviderRenewal{Capture: capture, Credential: id, Revision: value.Revision, Grant: previous}, nil
}

func CompleteProviderRenewal(ctx context.Context, db *sql.Conn, node protocol.NodeID, renewal ProviderRenewal, grant providers.AccountGrant) ([]protocol.Event, error) {
	saved, err := currentProvider(ctx, db, renewal.Capture.Provider.ID, nil)
	if err != nil {
		return nil, err
	}
	capture, err := captureProviderLogin(ctx, db, renewal.Capture.Provider, renewal.Capture.Attempt.ID)
	if err != nil {
		return nil, err
	}
	current := saved.Authorizations[renewal.Credential]
	if current.Revision != renewal.Revision || current.Key == nil || current.Authentication != renewal.Capture.Provider.Authentication ||
		saved.Provider.Authentication != renewal.Capture.Provider.Authentication || !reflect.DeepEqual(capture.Target, renewal.Capture.Target) || !renewal.Grant.SameAccount(grant) {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "provider authorization changed during renewal")
	}
	grant.RenewalPending = false
	body, err := json.Marshal(grant)
	if err != nil {
		return nil, err
	}
	secret := protocol.NewSecret(string(body))
	current.Key, current.ExpiresAtMs = &secret, grant.Expiry()
	current.Revision, err = nextRevision(current.Revision)
	if err != nil {
		return nil, err
	}
	saved.Authorizations[renewal.Credential] = current
	if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
		return nil, err
	}
	events := []protocol.Event{{Kind: "credential_changed", Data: protocol.EventCredentialChangedPayload{Credential: protocol.CredentialRef{Node: node, ID: renewal.Credential}, Revision: current.Revision}}}
	quotaChanges, err := ReconcileProviderQuotas(ctx, db, node)
	return append(events, quotaChanges...), err
}
