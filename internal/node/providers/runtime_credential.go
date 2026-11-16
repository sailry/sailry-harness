package providers

import (
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RuntimeAccount exports only fields used to execute an already selected model.
// Plugin extras enter the projection only through declared header injections.
func RuntimeAccount(provider protocol.ConversationProvider, grant AccountGrant) (protocol.RuntimeCredential, error) {
	value := protocol.RuntimeCredential{
		Authentication: grant.Kind, Provider: &provider.ID,
		Access: protocol.NewSecret(grant.Access), ExpiresAtMs: grant.Expiry(), IssuedAtMs: grant.IssuedAtMs,
		AccountID: grant.AccountID, InstallationID: grant.InstallationID, Scopes: append([]string(nil), grant.Scopes...),
	}
	switch grant.Kind {
	case protocol.AuthenticationChatGpt:
		value.Endpoint = "https://api.openai.com/v1"
	case protocol.AuthenticationChatGptOAuth:
		value.Endpoint = "https://chatgpt.com/backend-api/codex"
	case protocol.AuthenticationClaudeOAuth:
		value.Endpoint = "https://api.anthropic.com"
	case protocol.AuthenticationCopilot:
		value.Endpoint = grant.Endpoint
	}
	if grant.Kind != protocol.AuthenticationPlugin {
		return value, nil
	}
	if grant.Record == nil || grant.Record.Target.Provider != provider.ID {
		return value, invalid("plugin authorization does not match its provider")
	}
	target, issued := grant.Record.Target, grant.Record.Issued
	value.Package, value.Flow, value.Endpoint = &target.Package, target.Flow, target.Endpoint
	value.Access, value.IssuedAtMs = protocol.NewSecret(issued.Access), issued.IssuedAtMs
	// Fields used only during login or renewal can never be rendered into an
	// executor credential, even if an old declaration named one explicitly.
	fields := map[string]string{"access": issued.Access}
	for name, text := range issued.Extra {
		if RuntimeField(name) {
			fields["extra."+name] = text
		}
	}
	filtered := authorization.Issued{Access: issued.Access, Extra: map[string]string{}}
	for name, text := range fields {
		if strings.HasPrefix(name, "extra.") {
			filtered.Extra[strings.TrimPrefix(name, "extra.")] = text
		}
	}
	headers, err := filtered.Headers(target.Declaration.Inject)
	if err != nil {
		return value, invalid("plugin injection requires controller-only authorization material")
	}
	value.Headers = map[string]protocol.Secret{}
	for name, values := range headers {
		text := strings.Join(values, ", ")
		protected := []string{grant.Refresh, grant.IDToken, grant.Github, target.Fields["client_secret"]}
		if issued.Refresh != nil {
			protected = append(protected, *issued.Refresh)
		}
		for field, secret := range issued.Extra {
			if !RuntimeField(field) {
				protected = append(protected, secret)
			}
		}
		for _, secret := range protected {
			if secret != "" && strings.Contains(text, secret) {
				return value, invalid("plugin injection exposes controller-only authorization material")
			}
		}
		value.Headers[name] = protocol.NewSecret(text)
	}
	return value, nil
}

func RuntimeField(name string) bool {
	name = strings.ToLower(strings.NewReplacer("_", "", "-", "", ".", "").Replace(name))
	return !strings.Contains(name, "refresh") && !strings.Contains(name, "clientsecret") &&
		!strings.Contains(name, "idtoken") && !strings.Contains(name, "mcpoauth")
}

func RuntimeAccountSecret(value protocol.RuntimeCredential) (*protocol.Secret, error) {
	grant := AccountGrant{
		Kind: value.Authentication, Access: value.Access.Expose(), ExpiresAtMs: 0, IssuedAtMs: value.IssuedAtMs,
		AccountID: value.AccountID, InstallationID: value.InstallationID, Scopes: value.Scopes,
		Endpoint: value.Endpoint, Runtime: &value,
	}
	if value.ExpiresAtMs != nil {
		grant.ExpiresAtMs = *value.ExpiresAtMs
	}
	body, err := json.Marshal(grant)
	if err != nil {
		return nil, err
	}
	secret := protocol.NewSecret(string(body))
	return &secret, nil
}

func runtimeHeaders(provider protocol.ConversationProvider, grant AccountGrant) (http.Header, error) {
	value := grant.Runtime
	if value == nil || value.Provider == nil || *value.Provider != provider.ID ||
		strings.TrimRight(value.Endpoint, "/") != strings.TrimRight(provider.Endpoint, "/") ||
		value.Authentication != provider.Authentication {
		return nil, invalid("runtime credential does not match the captured provider")
	}
	if provider.Authentication != protocol.AuthenticationPlugin {
		return nil, nil
	}
	if provider.Binding == nil || *provider.Binding == nil || (**provider.Binding).Package == nil || (**provider.Binding).Auth == nil ||
		value.Package == nil || value.Package.Name != (**provider.Binding).Package.Name || value.Flow != (**provider.Binding).Auth.Flow {
		return nil, invalid("runtime credential does not match the captured authentication")
	}
	headers := http.Header{}
	for name, value := range value.Headers {
		headers.Set(name, value.Expose())
	}
	return headers, nil
}

// RenewalAt returns the next refresh time without inventing an unknown TTL.
// Legacy grants retain their former one-minute-before-expiry behavior.
func (grant AccountGrant) RenewalAt() *uint64 {
	expiry := grant.Expiry()
	if expiry == nil {
		return nil
	}
	issued := grant.IssuedAtMs
	if grant.Record != nil {
		issued = grant.Record.Issued.IssuedAtMs
	}
	due := uint64(0)
	if *expiry > uint64(time.Minute.Milliseconds()) {
		due = *expiry - uint64(time.Minute.Milliseconds())
	}
	if issued != nil && *issued < *expiry {
		due = *issued + (*expiry-*issued)/4
	}
	if grant.EarliestRefreshAt != nil && due < *grant.EarliestRefreshAt {
		due = *grant.EarliestRefreshAt
	}
	return &due
}
