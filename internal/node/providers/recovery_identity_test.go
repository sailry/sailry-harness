package providers

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRecoveryIdentityRequiresStableAccountAndWorkspace(t *testing.T) {
	t.Parallel()
	account, workspace, email := "account-a", "workspace-a", "same@example.test"
	grant := AccountGrant{Kind: protocol.AuthenticationChatGptOAuth, Subject: "subject-a", AccountID: &account, OrgID: &workspace, Email: &email}
	original := RecoveryAccountIdentity(grant)
	if original == "" {
		t.Fatal("service identity did not produce a recovery identity")
	}
	otherEmail := "changed@example.test"
	grant.Email, grant.Access, grant.Refresh = &otherEmail, "fresh-access", "fresh-refresh"
	if RecoveryAccountIdentity(grant) != original {
		t.Fatal("labels or rotated tokens changed stable account identity")
	}
	grant.Subject = "subject-b"
	if RecoveryAccountIdentity(grant) == original {
		t.Fatal("a different principal with the same email matched")
	}
	grant.Subject = "subject-a"
	otherWorkspace := "workspace-b"
	grant.OrgID = &otherWorkspace
	if RecoveryAccountIdentity(grant) == original {
		t.Fatal("a different workspace matched the original account scope")
	}
	for _, kind := range []protocol.Authentication{protocol.AuthenticationChatGptOAuth, protocol.AuthenticationClaudeOAuth, protocol.AuthenticationCopilot, protocol.AuthenticationPlugin} {
		if RecoveryAccountIdentity(AccountGrant{Kind: kind, Email: &email, Access: "same-access", Github: "same-github-token"}) != "" {
			t.Fatalf("%s accepted email or token equality without service identity", kind)
		}
	}
}

func TestRecoveryAuthenticationTargetSeparatesProviderAndClient(t *testing.T) {
	t.Parallel()
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Authentication, provider.Endpoint = protocol.NewProviderID(), protocol.AuthenticationChatGptOAuth, "https://service.example.test"
	grant := AccountGrant{Kind: provider.Authentication, ClientID: "client-a", Subject: "subject-a"}
	original := RecoveryAuthenticationTarget(provider, nil, grant)
	grant.ClientID = "client-b"
	if RecoveryAuthenticationTarget(provider, nil, grant) == original {
		t.Fatal("a different OAuth client matched")
	}
	grant.ClientID = "client-a"
	provider.Endpoint = "https://different.example.test"
	if RecoveryAuthenticationTarget(provider, nil, grant) == original {
		t.Fatal("a different authentication endpoint matched")
	}
}
