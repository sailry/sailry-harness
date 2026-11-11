//go:build integration

package node

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Two Nodes pair over the loopback Link. The first Node's local controller
// creates a session on the second with a provider only the first has, and a
// turn on the second authorizes with the forwarded credential.
func TestSessionForwardingIntegration(test *testing.T) {
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
	config.Provider, config.Model, config.Credential = provider.ID, "fixture", provider.Credential
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject

	stranger := protocol.NodeID{7}
	if _, err := local.Do(ctx, protocol.Command{Kind: "create_session_at", Data: protocol.CommandCreateSessionAtPayload{Target: stranger, Config: &config, ProviderRevision: provider.Revision}}); err == nil || !strings.Contains(err.Error(), "target Node is not paired") {
		test.Fatalf("forward to an unpaired Node: %v", err)
	}

	request := local.Prepare(protocol.Command{Kind: "create_session_at", Data: protocol.CommandCreateSessionAtPayload{Target: b.ID(), Config: &config, ProviderRevision: provider.Revision}})
	output, err := local.Execute(ctx, request)
	if err != nil {
		test.Fatal(err)
	}
	session, valid := dispatchPayload[protocol.Session](output.Data)
	if !valid || session.Profile == nil || session.Profile.Source != a.ID() || session.Config.Provider == provider.ID ||
		session.Config.Credential == nil || session.Config.Credential.Node != b.ID() {
		test.Fatalf("forwarded session = %#v", output.Data)
	}
	replay, err := local.Execute(ctx, request)
	replayed, valid := dispatchPayload[protocol.Session](replay.Data)
	if err != nil || !valid || replayed.ID != session.ID || len(snapshotFixture(test, b).Sessions) != 1 || len(snapshotFixture(test, a).Sessions) != 0 {
		test.Fatalf("forward replay: %v", err)
	}
	for name, node := range map[string]*Node{"source": a, "target": b} {
		if text := nodeText(test, node); strings.Contains(text, forwardedKey) {
			test.Fatalf("%s exposes the forwarded secret", name)
		}
	}

	remote := client.New(a.Link().Remote(address))
	defer remote.Close()
	if err := remote.Start(ctx); err != nil {
		test.Fatal(err)
	}
	input := protocol.DefaultConversationInput()
	input.Text = "Say hello"
	if _, err := remote.Do(ctx, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: input}}); err != nil {
		test.Fatal(err)
	}
	select {
	case authorization := <-authorizations:
		if authorization != "Bearer "+forwardedKey {
			test.Fatalf("model request authorization = %q", authorization)
		}
	case <-ctx.Done():
		test.Fatal(context.Cause(ctx))
	}
}
