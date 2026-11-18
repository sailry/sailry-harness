//go:build integration

package node

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Paired Nodes keep their own model channels while the controller can address
// a session on the execution Node.
func TestExecutionNodeOwnsSessionProviderIntegration(test *testing.T) {
	ctx, cancel := context.WithTimeout(test.Context(), 60*time.Second)
	defer cancel()
	authorizations := make(chan string, 4)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case authorizations <- r.Header.Get("Authorization"):
		default:
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	scope := link.NetworkScope{Bind: "127.0.0.1:0"}
	start := func(name string) *Node {
		node, err := StartWithOptions(ctx, filepath.Join(test.TempDir(), name), Options{Network: &scope, MCP: fixtureMCPOptions()})
		if err != nil {
			test.Fatal(err)
		}
		test.Cleanup(func() { _ = node.Stop(context.Background()) })
		return node
	}
	a, b := start("source"), start("target")
	invitation, err := b.Link().Invite(ctx)
	if err != nil {
		test.Fatal(err)
	}
	defer invitation.Close()
	address, err := a.Link().Pair(ctx, invitation.Ticket())
	if err != nil {
		test.Fatal(err)
	}
	if err := invitation.Paired(ctx); err != nil {
		test.Fatal(err)
	}

	local := client.New(link.NewLocal(a.ID(), a.ID(), a))
	defer local.Close()
	if err := local.Start(ctx); err != nil {
		test.Fatal(err)
	}
	provider := forwardingProvider(test, a, server.URL+"/v1")
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	stranger := protocol.NodeID{7}
	var unpairedFault *link.FaultError
	if _, err := local.Do(ctx, protocol.Command{Kind: "create_session_at", Data: protocol.CommandCreateSessionAtPayload{Target: stranger, Config: &config, ProviderRevision: provider.Revision}}); !errors.As(err, &unpairedFault) || unpairedFault.Fault.Code != protocol.ErrorCodeNotConfigured {
		test.Fatalf("the retired forwarding command accepted an unpaired target: %v", err)
	}

	remote := client.New(a.Link().Remote(address))
	defer remote.Close()
	if err := remote.Start(ctx); err != nil {
		test.Fatal(err)
	}
	request := local.Prepare(protocol.Command{Kind: "create_session_at", Data: protocol.CommandCreateSessionAtPayload{Target: b.ID(), Config: &config, ProviderRevision: provider.Revision}})
	_, err = local.Execute(ctx, request)
	var fault *link.FaultError
	if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodeNotConfigured {
		test.Fatalf("controller-owned model created a remote session: %v", err)
	}
	_, err = local.Execute(ctx, request)
	var replayFault *link.FaultError
	if !errors.As(err, &replayFault) || replayFault.Fault.Code != protocol.ErrorCodeNotConfigured {
		test.Fatalf("replayed rejected request changed its result: %v", err)
	}

	listed, err := remote.Do(ctx, protocol.Command{Kind: "list_providers"})
	if err != nil {
		test.Fatal(err)
	}
	providers, valid := dispatchPayload[[]protocol.ConversationProvider](listed.Data)
	if !valid || len(providers) != 0 {
		test.Fatalf("controller model appeared as an execution Node choice: %#v", listed.Data)
	}
	targetKey := "execution-node-key"
	localProvider := provider
	localProvider.Owner, localProvider.Source, localProvider.Consent = nil, nil, nil
	localProvider.Revision, localProvider.Shared = 0, false
	localProvider.Name, localProvider.Credential = "Execution channel", nil
	secret := protocol.NewSecret(targetKey)
	savedOutput, err := remote.Do(ctx, protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{
		Provider: localProvider, ExpectedRevision: 0, Secret: &secret,
	}})
	if err != nil {
		test.Fatal(err)
	}
	saved, valid := dispatchPayload[protocol.ConversationProvider](savedOutput.Data)
	if !valid || saved.ID != provider.ID || saved.Credential == nil || saved.Credential.Node != b.ID() {
		test.Fatalf("execution Node provider was not saved independently: %#v", savedOutput.Data)
	}
	config.Provider, config.Model, config.Credential = saved.ID, saved.DefaultModel, saved.Credential
	created, err := remote.Do(ctx, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config}})
	if err != nil {
		test.Fatal(err)
	}
	session, valid := dispatchPayload[protocol.Session](created.Data)
	if !valid || session.Profile != nil || session.Config.Credential == nil || session.Config.Credential.Node != b.ID() {
		test.Fatalf("session did not capture its execution Node provider: %#v", created.Data)
	}
	projection, err := remote.Do(ctx, protocol.Command{Kind: "snapshot"})
	visible, ok := dispatchPayload[protocol.Snapshot](projection.Data)
	if err != nil || !ok || len(visible.Sessions) != 1 || visible.Sessions[0].ID != session.ID || len(snapshotFixture(test, a).Sessions) != 0 {
		test.Fatalf("execution Node session projection: %+v: %v", projection, err)
	}
	input := protocol.DefaultConversationInput()
	input.Text = "Say hello"
	if _, err := remote.Do(ctx, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}}); err != nil {
		test.Fatal(err)
	}
	select {
	case authorization := <-authorizations:
		if authorization != "Bearer "+targetKey {
			test.Fatalf("model request authorization = %q", authorization)
		}
	case <-ctx.Done():
		test.Fatal(context.Cause(ctx))
	}
}
