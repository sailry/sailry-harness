package node

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A confidential OAuth client renews with its secret; a public one has none
// to send.
func TestConnectorRefreshClientSecret(t *testing.T) {
	for _, confidential := range []bool{true, false} {
		client := protocol.PluginConnectorClientFields{ClientID: "client"}
		record := store.ConnectorRecord{Method: protocol.PluginConnectorKindOauth2, Fields: map[string]string{"client": "fixture-client"}}
		if confidential {
			name := "secret"
			client.ClientSecret = &name
			record.Secrets = map[string]protocol.Secret{"secret": protocol.NewSecret("fixture-secret")}
		}
		capture := store.ConnectorLoginCapture{Record: record, Declaration: protocol.PluginConnectorDeclaration{
			ID: "account", Origins: []string{"https://fixture.test"},
			Auth: []protocol.PluginConnectorAuth{{Kind: "oauth2", Data: protocol.PluginConnectorAuthOauth2Payload{
				Flow: protocol.PluginConnectorFlowPkce, Authorize: "https://fixture.test/authorize", Token: "https://fixture.test/token", Refresh: true,
				Client: protocol.PluginConnectorClient{Kind: "Fields", Data: protocol.PluginConnectorClientFieldsPayload{Fields: client}},
			}}},
		}}
		declaration, fields, err := connectorModelAuth(capture, false)
		if err != nil || declaration.Refresh == nil {
			t.Fatalf("refresh declaration: %v", err)
		}
		form, ok := dispatchPayload[protocol.PluginModelAuthBodyFormPayload](declaration.Refresh.Request.Body.Data)
		if !ok || form.Fields["client_id"] != "{client_id}" {
			t.Fatalf("refresh form: %+v", form)
		}
		if _, sent := form.Fields["client_secret"]; sent != confidential || confidential && (form.Fields["client_secret"] != "{client_secret}" || fields["client_secret"] != "fixture-secret") {
			t.Fatalf("confidential %v refresh form: %+v", confidential, form.Fields)
		}
	}
}
