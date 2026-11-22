//go:build integration

package link

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPairPINConnectsAcrossDifferentRelays(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
	defer cancel()
	hostRelay, hostCounter := startLibp2pTestRelay(t, 11)
	clientRelay, clientCounter := startLibp2pTestRelay(t, 12)
	if hostRelay == clientRelay {
		t.Fatal("relay fixtures unexpectedly share an address")
	}

	hostIdentity, err := OpenIdentity(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	clientIdentity, err := OpenIdentity(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	hostHandler := fixtureHandler{dispatch: func(_ context.Context, caller protocol.NodeID, request protocol.Request) (Admission, error) {
		if caller != clientIdentity.ID() || request.Target != hostIdentity.ID() {
			t.Error("cross-relay request lost its authenticated caller or target")
		}
		completed := make(chan Response, 1)
		completed <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
		return Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
	}}
	host := bindLibp2pTestNode(t, ctx, NetworkScope{Bind: "127.0.0.1:0", Relays: []string{hostRelay}}, hostIdentity, hostHandler)
	client := bindLibp2pTestNode(t, ctx, NetworkScope{Bind: "127.0.0.1:0", Relays: []string{clientRelay}}, clientIdentity, nil)
	if err := host.Online(ctx); err != nil {
		t.Fatal(err)
	}
	if err := client.Online(ctx); err != nil {
		t.Fatal(err)
	}

	circuit := hostRelay + "/p2p-circuit"
	service := newPairingExchangeFixture(t, circuit)
	sharing, err := host.Share(ctx, service.URL)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = sharing.Close() })
	var code string
	for {
		state, err := sharing.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		switch state.Kind {
		case "preparing":
			continue
		case "ready":
			code = state.Code
		case "retrying":
			t.Fatalf("pairing service retried unexpectedly: %v", state.Fault)
		default:
			t.Fatalf("Host sharing entered unexpected state %q", state.Kind)
		}
		break
	}

	selection := client.NetworkSelection()
	if selection == nil {
		t.Fatal("client did not retain its own selected relay")
	}
	clientCircuits := clientCounter.connects.Load()
	var claim PairingClaim
	hostAddress, err := client.PairPIN(ctx, service.URL, code, &claim)
	if err != nil {
		t.Fatalf("claim on another Host relay failed: %v", err)
	}
	if hostAddress.ID != hostIdentity.ID() {
		t.Fatal("pairing connected to a different Host")
	}
	var hint struct{ Libp2p string }
	if len(hostAddress.Addrs) != 1 || json.Unmarshal(hostAddress.Addrs[0], &hint) != nil || hint.Libp2p != circuit {
		t.Fatal("paired Host address did not retain its relay route")
	}
	after := client.NetworkSelection()
	beforeJSON, beforeErr := json.Marshal(selection)
	afterJSON, afterErr := json.Marshal(after)
	if after == nil || beforeErr != nil || afterErr != nil || !bytes.Equal(beforeJSON, afterJSON) {
		t.Fatal("pairing changed the client's selected relay network")
	}

	request := protocol.NewRequest(hostAddress.ID, protocol.Command{Kind: "snapshot"})
	admission, err := client.Remote(hostAddress).Dispatch(ctx, request)
	if err != nil || admission.Receipt.ID != request.ID {
		t.Fatalf("request over the Host relay was not admitted: %#v, %v", admission, err)
	}
	if result := <-admission.Completion; result.Fault != nil || result.Output.Kind != "notifications_updated" {
		t.Fatalf("request over the Host relay did not complete: %#v", result)
	}
	assertLibp2pTestPhysicalPath(t, client, hostAddress.ID, "relay")
	paths := client.endpoint.(*libp2pEndpoint).host.Network().ConnsToPeer(libp2pTestPeerID(t, hostAddress.ID))
	for _, path := range paths {
		if !path.IsClosed() && path.RemoteMultiaddr().String() == circuit && hostCounter.connects.Load() > 0 && clientCounter.connects.Load() == clientCircuits {
			return
		}
	}
	t.Fatal("cross-relay pairing did not use the Host's public relay")
}

func newPairingExchangeFixture(t *testing.T, circuit string) *httptest.Server {
	t.Helper()
	var mu sync.Mutex
	var ticket string
	var expiresAt uint64
	var cancelToken string
	const code = "493243"
	server := httptest.NewServer(http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodPost {
			http.Error(response, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		switch request.URL.Path {
		case "/v1/codes":
			var publication struct {
				Ticket      string `json:"ticket"`
				ExpiresAtMS uint64 `json:"expires_at_ms"`
			}
			if json.NewDecoder(request.Body).Decode(&publication) != nil || publication.Ticket == "" {
				http.Error(response, "invalid publication", http.StatusBadRequest)
				return
			}
			var offer pairingTicket
			if err := json.Unmarshal([]byte(publication.Ticket), &offer); err != nil {
				http.Error(response, "invalid invitation", http.StatusBadRequest)
				return
			}
			// Remove the loopback hints so the claimant must use the circuit
			// advertised by the Host. The identity and ticket token are unchanged.
			var relayHint json.RawMessage
			for _, raw := range offer.Address.Addrs {
				var hint struct{ Libp2p string }
				if json.Unmarshal(raw, &hint) == nil && hint.Libp2p == circuit {
					relayHint = raw
					break
				}
			}
			if relayHint == nil {
				http.Error(response, "invitation did not advertise the Host circuit", http.StatusBadRequest)
				return
			}
			offer.Address.Addrs = []json.RawMessage{relayHint}
			encoded, err := json.Marshal(offer)
			if err != nil {
				http.Error(response, "cannot encode invitation", http.StatusInternalServerError)
				return
			}
			identifier, err := newPairingRequestID(time.Now())
			if err != nil {
				http.Error(response, "identifier unavailable", http.StatusInternalServerError)
				return
			}
			mu.Lock()
			ticket, expiresAt, cancelToken = string(encoded), publication.ExpiresAtMS, identifier
			mu.Unlock()
			response.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(response).Encode(map[string]any{"code": code, "expires_at_ms": publication.ExpiresAtMS, "cancel_token": identifier})
		case "/v1/codes/redeem":
			var redemption struct {
				Code string `json:"code"`
			}
			if json.NewDecoder(request.Body).Decode(&redemption) != nil || redemption.Code != code {
				http.Error(response, "code not found", http.StatusNotFound)
				return
			}
			mu.Lock()
			currentTicket, currentExpiry := ticket, expiresAt
			mu.Unlock()
			if currentTicket == "" || currentExpiry <= uint64(time.Now().UnixMilli()) {
				http.Error(response, "code expired", http.StatusGone)
				return
			}
			_ = json.NewEncoder(response).Encode(map[string]any{"ticket": currentTicket, "expires_at_ms": currentExpiry})
		case "/v1/codes/cancel":
			var cancellation struct {
				Code        string `json:"code"`
				CancelToken string `json:"cancel_token"`
			}
			mu.Lock()
			currentCancelToken := cancelToken
			mu.Unlock()
			if json.NewDecoder(request.Body).Decode(&cancellation) != nil || cancellation.CancelToken != currentCancelToken {
				http.Error(response, "cancel rejected", http.StatusForbidden)
				return
			}
			response.WriteHeader(http.StatusNoContent)
		default:
			http.NotFound(response, request)
		}
	}))
	t.Cleanup(server.Close)
	return server
}
