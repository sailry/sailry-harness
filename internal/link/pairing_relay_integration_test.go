//go:build integration

package link

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPairPINConnectsAcrossDifferentRelays(t *testing.T) {
	if err := iroh.LoadDefault(true); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
	defer cancel()
	hostRelay := startPairingRelayFixture(t)
	clientRelay := startPairingRelayFixture(t)
	if hostRelay.URL == clientRelay.URL {
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
	var host *Link
	hostHandler := fixtureHandler{dispatch: func(_ context.Context, caller protocol.NodeID, request protocol.Request) (Admission, error) {
		if caller != clientIdentity.ID() || request.Target != hostIdentity.ID() {
			t.Error("cross-relay request lost its authenticated caller or target")
		}
		completed := make(chan Response, 1)
		completed <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
		return Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
	}}
	host = bindPairingRelayEndpoint(t, ctx, hostIdentity, hostRelay, []relayFixture{hostRelay, clientRelay}, hostHandler)
	client := bindPairingRelayEndpoint(t, ctx, clientIdentity, clientRelay, []relayFixture{clientRelay, hostRelay}, nil)
	if err := host.Online(ctx); err != nil {
		t.Fatal(err)
	}
	if err := client.Online(ctx); err != nil {
		t.Fatal(err)
	}

	service := newPairingExchangeFixture(t)
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
	var claim PairingClaim
	hostAddress, err := client.PairPIN(ctx, service.URL, code, &claim)
	if err != nil {
		t.Fatalf("claim on another Host relay failed: %v", err)
	}
	if hostAddress.ID != hostIdentity.ID() {
		t.Fatal("pairing connected to a different Host")
	}
	if !containsRelay(addressRelayURLs(hostAddress), canonicalFixtureRelay(hostRelay.URL)) {
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
}

type relayFixture struct {
	URL      string     `json:"url"`
	RootsDER [][]uint16 `json:"roots_der"`
}

func bindPairingRelayEndpoint(t *testing.T, ctx context.Context, identity Identity, selected relayFixture, trusted []relayFixture, handler Handler) *Link {
	t.Helper()
	roots := make([][]byte, 0, len(trusted))
	for _, fixture := range trusted {
		for _, encoded := range fixture.RootsDER {
			root := make([]byte, len(encoded))
			for index, value := range encoded {
				if value > 255 {
					t.Fatal("relay fixture returned an invalid certificate byte")
				}
				root[index] = byte(value)
			}
			roots = append(roots, root)
		}
	}
	store := &fixturePeerStore{trusted: map[protocol.NodeID]bool{}, directions: map[protocol.NodeID]PeerDirection{}, addresses: map[protocol.NodeID]EndpointAddress{}}
	endpoint, err := bindConfigured(ctx, EndpointOptions{
		Identity: identity, Scope: NetworkScope{Relays: []string{selected.URL}}, Peers: store, Handler: handler,
	}, iroh.Config{
		SecretKey: identity.SecretKey(), Mode: "custom", Relays: []string{selected.URL}, RootsDER: roots, RelayOnly: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = endpoint.Close() })
	return endpoint
}

func startPairingRelayFixture(t *testing.T) relayFixture {
	t.Helper()
	path := os.Getenv("SAILRY_RELAY_FIXTURE")
	if path == "" {
		name := "relay-fixture"
		if runtime.GOOS == "windows" {
			name += ".exe"
		}
		directory, err := os.Getwd()
		if err != nil {
			t.Fatal(err)
		}
		for {
			candidate := filepath.Join(directory, ".runtime", "native", runtime.GOOS+"-"+runtime.GOARCH, name)
			if _, err := os.Stat(candidate); err == nil {
				path = candidate
				break
			}
			parent := filepath.Dir(directory)
			if parent == directory {
				t.Skip("build native/iroh with --fixtures or set SAILRY_RELAY_FIXTURE")
			}
			directory = parent
		}
	}

	command := exec.Command(path)
	stdin, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var stderr bytes.Buffer
	command.Stderr = &stderr
	if err := command.Start(); err != nil {
		t.Fatalf("start relay fixture: %v", err)
	}
	done := false
	t.Cleanup(func() {
		if done {
			return
		}
		_, _ = io.WriteString(stdin, "{\"op\":\"close\"}\n")
		_ = stdin.Close()
		if err := command.Wait(); err != nil {
			t.Errorf("relay fixture exited: %v: %s", err, stderr.String())
		}
		done = true
	})

	type initialResult struct {
		line []byte
		err  error
	}
	result := make(chan initialResult, 1)
	go func() {
		line, err := bufio.NewReader(stdout).ReadBytes('\n')
		result <- initialResult{line: line, err: err}
	}()
	select {
	case initial := <-result:
		if initial.err != nil {
			t.Fatalf("read relay fixture address: %v: %s", initial.err, stderr.String())
		}
		var fixture relayFixture
		if err := json.Unmarshal(initial.line, &fixture); err != nil || fixture.URL == "" || len(fixture.RootsDER) == 0 {
			t.Fatalf("invalid relay fixture startup response: %v", err)
		}
		return fixture
	case <-time.After(10 * time.Second):
		_ = command.Process.Kill()
		_ = command.Wait()
		done = true
		t.Fatalf("relay fixture did not become ready: %s", stderr.String())
		return relayFixture{}
	}
}

func newPairingExchangeFixture(t *testing.T) *httptest.Server {
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
			identifier, err := newPairingRequestID(time.Now())
			if err != nil {
				http.Error(response, "identifier unavailable", http.StatusInternalServerError)
				return
			}
			mu.Lock()
			ticket, expiresAt, cancelToken = publication.Ticket, publication.ExpiresAtMS, identifier
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
			if json.NewDecoder(request.Body).Decode(&cancellation) != nil || cancellation.Code != code || cancellation.CancelToken != currentCancelToken {
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

func canonicalFixtureRelay(value string) string {
	canonical, err := canonicalRelayURL(value)
	if err != nil {
		return fmt.Sprintf("invalid relay fixture URL: %v", err)
	}
	return canonical
}
