package providers

import (
	"encoding/json"
	"net/http"
	"net/url"
	"reflect"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
)

const DefaultCatalogVersion = "0.160.1"
const DefaultClaudeAgent = "claude-cli/2.1.280 (external, cli)"

// Copilot's service identifies its editor client by these values.
const (
	DefaultCopilotAgent        = "GitHubCopilotChat/0.68.0"
	DefaultCopilotEditor       = "vscode/1.140.0"
	DefaultCopilotEditorPlugin = "copilot-chat/0.68.0"
	copilotAPIVersion          = "2026-01-09"
)

type AccountTarget struct {
	Provider    protocol.ProviderID                 `json:"provider"`
	Package     protocol.PluginReference            `json:"package"`
	Origin      protocol.ConversationConsentOrigin  `json:"origin"`
	Flow        string                              `json:"flow"`
	Endpoint    string                              `json:"endpoint"`
	Declaration protocol.PluginModelAuthDeclaration `json:"declaration"`
	Fields      map[string]string                   `json:"fields"`
}

type AccountRecord struct {
	Target AccountTarget        `json:"target"`
	Issued authorization.Issued `json:"issued"`
}

type AccountGrant struct {
	Kind              protocol.Authentication `json:"kind"`
	Access            string                  `json:"access,omitempty"`
	Refresh           string                  `json:"refresh,omitempty"`
	ExpiresAtMs       uint64                  `json:"expires_at_ms,omitempty"`
	EarliestRefreshAt *uint64                 `json:"earliest_refresh_at,omitempty"`
	AccountID         *string                 `json:"account_id,omitempty"`
	ClientID          string                  `json:"client_id,omitempty"`
	Subject           string                  `json:"subject,omitempty"`
	Email             *string                 `json:"email,omitempty"`
	IDToken           string                  `json:"id_token,omitempty"`
	Scopes            []string                `json:"scopes,omitempty"`
	OrgID             *string                 `json:"org_id,omitempty"`
	OrgName           *string                 `json:"org_name,omitempty"`
	InstallationID    *string                 `json:"installation_id,omitempty"`
	Github            string                  `json:"github,omitempty"`
	Endpoint          string                  `json:"endpoint,omitempty"`
	Record            *AccountRecord          `json:"record,omitempty"`
	RenewalPending    bool                    `json:"renewal_pending"`
	RenewalRejected   bool                    `json:"renewal_rejected,omitempty"`
}

func (target AccountTarget) Matches(current AccountTarget) bool {
	clientFields := func(fields map[string]string) map[string]string {
		result := map[string]string{}
		for _, name := range []string{"client_id", "client_secret"} {
			if value, exists := fields[name]; exists {
				result[name] = value
			}
		}
		return result
	}
	refresh := func(declaration protocol.PluginModelAuthDeclaration) *protocol.PluginModelAuthEndpoint {
		if declaration.Refresh == nil {
			return nil
		}
		return &declaration.Refresh.Request
	}
	sameOrigin := reflect.DeepEqual(target.Origin, current.Origin) || target.Origin.Kind == "imported" && current.Origin.Kind == "imported"
	return target.Provider == current.Provider && target.Package.Name == current.Package.Name && sameOrigin &&
		target.Flow == current.Flow && target.Endpoint == current.Endpoint &&
		reflect.DeepEqual(target.Declaration.Flow, current.Declaration.Flow) &&
		reflect.DeepEqual(refresh(target.Declaration), refresh(current.Declaration)) &&
		reflect.DeepEqual(target.Declaration.Inject, current.Declaration.Inject) &&
		reflect.DeepEqual(target.Declaration.Scopes, current.Declaration.Scopes) &&
		reflect.DeepEqual(clientFields(target.Fields), clientFields(current.Fields))
}

func OAuthFields(provider protocol.ConversationProvider) (map[string]string, error) {
	fields := map[string]string{"user_agent": "Sailry/0.1"}
	if provider.Authentication == protocol.AuthenticationApiKey {
		if provider.Oauth != nil {
			return nil, invalid("API-key providers do not use OAuth settings")
		}
		return fields, nil
	}
	if provider.Authentication == protocol.AuthenticationPlugin {
		if provider.Binding == nil || *provider.Binding == nil || (**provider.Binding).Package == nil || (**provider.Binding).Auth == nil {
			return nil, invalid("plugin authentication requires a captured package flow")
		}
		return fields, nil
	}
	switch provider.Authentication {
	case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
		if provider.Api != protocol.ConversationModelApiResponses {
			return nil, invalid("OAuth settings do not match the provider protocol")
		}
	case protocol.AuthenticationClaudeOAuth:
		if provider.Api != protocol.ConversationModelApiAnthropic {
			return nil, invalid("OAuth settings do not match the provider protocol")
		}
		fields["user_agent"] = DefaultClaudeAgent
	case protocol.AuthenticationCopilot:
		if provider.Api != protocol.ConversationModelApiResponses && provider.Api != protocol.ConversationModelApiChatCompletions {
			return nil, invalid("OAuth settings do not match the provider protocol")
		}
		fields["user_agent"], fields["editor_version"], fields["editor_plugin_version"] = DefaultCopilotAgent, DefaultCopilotEditor, DefaultCopilotEditorPlugin
	default:
		return nil, invalid("provider authentication is unsupported")
	}
	if provider.Authentication == protocol.AuthenticationChatGptOAuth {
		fields["catalog_version"] = DefaultCatalogVersion
	}
	if provider.Oauth != nil {
		if provider.Oauth.Kind != string(provider.Authentication) {
			return nil, invalid("OAuth settings do not match the provider authentication")
		}
		var options map[string]json.RawMessage
		body, err := json.Marshal(provider.Oauth)
		if err != nil || json.Unmarshal(body, &options) != nil {
			return nil, invalid("OAuth settings are invalid")
		}
		for _, name := range []string{"user_agent", "editor_version", "editor_plugin_version", "catalog_version"} {
			if value, ok := options[name]; ok && string(value) != "null" {
				var text string
				if json.Unmarshal(value, &text) != nil {
					return nil, invalid("OAuth setting is invalid")
				}
				fields[name] = text
			}
		}
	}
	for name, value := range fields {
		if strings.TrimSpace(value) == "" || len(value) > 256 || strings.ContainsFunc(value, unicode.IsControl) || !httpguts.ValidHeaderFieldValue(value) {
			return nil, invalid("OAuth header value is invalid")
		}
		if name == "catalog_version" {
			parts := strings.Split(value, ".")
			if len(parts) != 3 {
				return nil, invalid("catalog version must use major.minor.patch")
			}
			for _, part := range parts {
				if part == "" || len(part) > 8 || strings.ContainsFunc(part, func(character rune) bool { return character < '0' || character > '9' }) {
					return nil, invalid("catalog version must use major.minor.patch")
				}
			}
		}
	}
	if provider.Authentication == protocol.AuthenticationClaudeOAuth && (!strings.HasPrefix(strings.ToLower(fields["user_agent"]), "claude-cli") || fields["user_agent"] == "claude-cli/2.1.280") {
		fields["user_agent"] = DefaultClaudeAgent
	}
	return fields, nil
}

func DecodeAccount(secret *protocol.Secret, authentication protocol.Authentication) (AccountGrant, error) {
	var grant AccountGrant
	if secret == nil || json.Unmarshal([]byte(secret.Expose()), &grant) != nil || grant.Kind != authentication {
		return grant, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "provider sign-in is required"}
	}
	if grant.RenewalPending {
		return grant, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "authorization refresh did not complete; sign in again"}
	}
	if grant.RenewalRejected {
		return grant, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "authorization refresh was rejected; sign in again"}
	}
	return grant, nil
}

func (grant AccountGrant) Expiry() *uint64 {
	if grant.Record != nil {
		return grant.Record.Issued.ExpiresAtMs
	}
	if grant.ExpiresAtMs == 0 {
		return nil
	}
	value := grant.ExpiresAtMs
	return &value
}

func (grant AccountGrant) Label() *string {
	label := grant.Email
	if grant.Record != nil {
		label = grant.Record.Issued.AccountLabel
	}
	if label == nil {
		return nil
	}
	value := strings.TrimSpace(*label)
	if value == "" || len(value) > 1024 || strings.ContainsFunc(value, unicode.IsControl) {
		return nil
	}
	return &value
}

func (grant AccountGrant) Secrets() []protocol.Secret {
	result := []protocol.Secret{}
	add := func(value string) {
		if value != "" {
			result = append(result, protocol.NewSecret(value))
		}
	}
	for _, value := range []string{grant.Access, grant.Refresh, grant.IDToken, grant.Github} {
		add(value)
	}
	if grant.AccountID != nil {
		add(*grant.AccountID)
	}
	if grant.Record != nil {
		add(grant.Record.Issued.Access)
		if grant.Record.Issued.Refresh != nil {
			add(*grant.Record.Issued.Refresh)
		}
		for _, value := range grant.Record.Issued.Extra {
			add(value)
		}
		for _, value := range grant.Record.Target.Fields {
			add(value)
		}
	}
	return result
}

func (grant AccountGrant) SameAccount(other AccountGrant) bool {
	if grant.Kind != other.Kind {
		return false
	}
	switch grant.Kind {
	case protocol.AuthenticationPlugin:
		return grant.Record != nil && other.Record != nil && grant.Record.Target.Matches(other.Record.Target)
	case protocol.AuthenticationChatGpt:
		return grant.ClientID == other.ClientID && grant.Subject == other.Subject
	case protocol.AuthenticationChatGptOAuth:
		return grant.Subject == other.Subject && reflect.DeepEqual(grant.AccountID, other.AccountID)
	case protocol.AuthenticationClaudeOAuth:
		return (other.AccountID == nil || reflect.DeepEqual(grant.AccountID, other.AccountID)) &&
			(other.OrgID == nil || reflect.DeepEqual(grant.OrgID, other.OrgID))
	case protocol.AuthenticationCopilot:
		return grant.Github == other.Github
	}
	return false
}

func AccountHeaders(provider protocol.ConversationProvider, secret *protocol.Secret) (AccountGrant, http.Header, error) {
	grant, err := DecodeAccount(secret, provider.Authentication)
	if err != nil {
		return grant, nil, err
	}
	if expiry := grant.Expiry(); expiry != nil && *expiry <= uint64(time.Now().UnixMilli()) {
		return grant, nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization expired; sign in again"}
	}
	fields, err := OAuthFields(provider)
	if err != nil {
		return grant, nil, err
	}
	if grant.Kind == protocol.AuthenticationPlugin {
		if grant.Record == nil || grant.Record.Target.Provider != provider.ID || grant.Record.Target.Endpoint != provider.Endpoint ||
			provider.Binding == nil || *provider.Binding == nil || (**provider.Binding).Package == nil || (**provider.Binding).Auth == nil ||
			grant.Record.Target.Package.Name != (**provider.Binding).Package.Name || grant.Record.Target.Flow != (**provider.Binding).Auth.Flow {
			return grant, nil, invalid("plugin authorization does not match its captured provider")
		}
		headers, err := grant.Record.Issued.Headers(grant.Record.Target.Declaration.Inject)
		return grant, headers, err
	}
	destination := ""
	switch grant.Kind {
	case protocol.AuthenticationChatGpt:
		destination = "https://api.openai.com/v1"
	case protocol.AuthenticationChatGptOAuth:
		destination = "https://chatgpt.com/backend-api/codex"
	case protocol.AuthenticationClaudeOAuth:
		destination = "https://api.anthropic.com"
	case protocol.AuthenticationCopilot:
		destination = grant.Endpoint
		target, err := Endpoint(destination)
		if err != nil || target.Scheme != "https" || (target.Hostname() != "api.githubcopilot.com" && !strings.HasSuffix(target.Hostname(), ".githubcopilot.com")) {
			return grant, nil, invalid("Copilot authorization endpoint is invalid")
		}
	}
	if strings.TrimRight(provider.Endpoint, "/") != strings.TrimRight(destination, "/") || grant.Access == "" || !httpguts.ValidHeaderFieldValue(grant.Access) {
		return grant, nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization destination is not authorized"}
	}
	headers := http.Header{"Authorization": {"Bearer " + grant.Access}, "User-Agent": {fields["user_agent"]}}
	switch grant.Kind {
	case protocol.AuthenticationChatGpt:
		for _, required := range []string{"resource.invoke", "chatgpt.tokens.use.direct"} {
			found := false
			for _, scope := range grant.Scopes {
				found = found || scope == required
			}
			if !found {
				return grant, nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "ChatGPT plan use is not enabled"}
			}
		}
	case protocol.AuthenticationChatGptOAuth:
		if grant.AccountID == nil || !httpguts.ValidHeaderFieldValue(*grant.AccountID) {
			return grant, nil, invalid("authorization account identifier is invalid")
		}
		headers.Set("originator", "codex_cli_rs")
		headers.Set("version", fields["catalog_version"])
		headers.Set("chatgpt-account-id", *grant.AccountID)
	case protocol.AuthenticationCopilot:
		headers.Set("editor-version", fields["editor_version"])
		headers.Set("editor-plugin-version", fields["editor_plugin_version"])
		headers.Set("copilot-integration-id", "vscode-chat")
		headers.Set("x-github-api-version", copilotAPIVersion)
	}
	return grant, headers, nil
}

func AccountResourceEndpoint(authentication protocol.Authentication, value string) bool {
	origin := ""
	switch authentication {
	case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
		origin = "https://chatgpt.com"
	case protocol.AuthenticationClaudeOAuth:
		origin = "https://api.anthropic.com"
	default:
		return false
	}
	target, err := url.Parse(value)
	return err == nil && target.User == nil && target.Fragment == "" && target.Scheme+"://"+target.Host == origin
}

func AccountResourceHeaders(provider protocol.ConversationProvider, secret *protocol.Secret, target string) (AccountGrant, http.Header, error) {
	if provider.Authentication == protocol.AuthenticationPlugin {
		return AccountHeaders(provider, secret)
	}
	grant, err := DecodeAccount(secret, provider.Authentication)
	if err != nil {
		return grant, nil, err
	}
	if !AccountResourceEndpoint(provider.Authentication, target) || grant.Access == "" || !httpguts.ValidHeaderFieldValue(grant.Access) {
		return grant, nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider resource origin is not authorized"}
	}
	headers := http.Header{"Authorization": {"Bearer " + grant.Access}}
	if (grant.Kind == protocol.AuthenticationChatGpt || grant.Kind == protocol.AuthenticationChatGptOAuth) && grant.AccountID != nil {
		if !httpguts.ValidHeaderFieldValue(*grant.AccountID) {
			return grant, nil, invalid("authorization account identifier is invalid")
		}
		headers.Set("chatgpt-account-id", *grant.AccountID)
	}
	return grant, headers, nil
}
