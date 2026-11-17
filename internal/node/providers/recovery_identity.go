package providers

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func identityDigest(value any) string {
	body, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	digest := sha256.Sum256(body)
	return hex.EncodeToString(digest[:])
}

// RecoveryAccountIdentity names only service-issued principal and workspace
// identifiers. Missing identifiers cannot be replaced by an email, a label or
// a token: those do not prove that a fresh login owns the original task.
func RecoveryAccountIdentity(grant AccountGrant) string {
	nonempty := func(value *string) bool { return value != nil && *value != "" }
	switch grant.Kind {
	case protocol.AuthenticationChatGpt:
		if grant.Subject == "" {
			return ""
		}
	case protocol.AuthenticationChatGptOAuth:
		if grant.Subject == "" || !nonempty(grant.AccountID) {
			return ""
		}
	case protocol.AuthenticationClaudeOAuth:
		if !nonempty(grant.AccountID) || !nonempty(grant.OrgID) {
			return ""
		}
	case protocol.AuthenticationCopilot, protocol.AuthenticationPlugin:
		if grant.Subject == "" {
			return ""
		}
	default:
		return ""
	}
	return identityDigest(struct {
		Kind               protocol.Authentication
		Subject            string
		Account, Workspace *string
	}{grant.Kind, grant.Subject, grant.AccountID, grant.OrgID})
}

// RecoveryAuthenticationTarget excludes installation-specific package digests
// while retaining the auth origin, flow and service/client target. The digest
// is an identity comparison, never an authorization or renewal credential.
func RecoveryAuthenticationTarget(provider protocol.ConversationProvider, target *AccountTarget, grant AccountGrant) string {
	var declaration any
	if target != nil {
		declaration = struct {
			Package        string
			Origin         any
			Flow, Endpoint string
			Declaration    protocol.PluginModelAuthDeclaration
			ClientID       string
		}{target.Package.Name, target.Origin, target.Flow, target.Endpoint, target.Declaration, target.Fields["client_id"]}
	}
	return identityDigest(struct {
		Provider           protocol.ProviderID
		Authentication     protocol.Authentication
		Endpoint, ClientID string
		Target             any
	}{provider.ID, provider.Authentication, provider.Endpoint, grant.ClientID, declaration})
}
