package link

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type pairingRoundTripper func(*http.Request) (*http.Response, error)

func (transport pairingRoundTripper) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func TestPairingExchangeContract(t *testing.T) {
	t.Parallel()
	now := time.Unix(100, 0)
	clock := func() time.Time { return now }
	for _, origin := range []string{
		"http://example.com", "http://localhost:1234", "https://user:secret@example.com",
		"https://example.com/?token=x", "https://example.com/#secret", "https://example.com/path",
	} {
		if _, err := newPairingRelay(origin, clock); err == nil {
			t.Fatalf("unsafe pairing origin accepted: %s", origin)
		}
	}
	for _, origin := range []string{"https://example.com", "http://127.0.0.1:1234", "http://[::1]:1234"} {
		relay, err := newPairingRelay(origin, clock)
		if err != nil {
			t.Fatal(err)
		}
		if relay.client.Timeout != 10*time.Second {
			t.Fatal("pairing exchange changed its source timeout")
		}
		transport := relay.client.Transport.(*http.Transport)
		if !transport.DisableKeepAlives || transport.ForceAttemptHTTP2 || len(transport.TLSNextProto) != 0 {
			t.Fatal("pairing exchange permits automatic transport replay")
		}
		relay.client.CloseIdleConnections()
	}
	for _, pin := range []string{"12345", "1234567", "１２３４５６", " 12345"} {
		if validPairingPIN(pin) {
			t.Fatalf("invalid PIN accepted: %s", pin)
		}
	}
	var claim PairingClaim
	first, err := claim.requestID("https://link.example/", "001234", now)
	if err != nil || !validPairingRequestID(first) {
		t.Fatalf("pairing request: %v", err)
	}
	again, err := claim.requestID("https://link.example/", "001234", now.Add(time.Second))
	if err != nil || again != first {
		t.Fatal("uncertain claim lost its original operation identity")
	}
	changed, err := claim.requestID("https://link.example/", "654321", now)
	if err != nil || changed == first {
		t.Fatal("changed intent reused another PIN's operation identity")
	}
	canonical, err := newPairingRelay("https://LINK.example:443/", clock)
	if err != nil || canonical.base.String() != "https://link.example/" {
		t.Fatalf("canonical pairing origin: %v", err)
	}
	canonical.client.CloseIdleConnections()
	t.Run("selected_pairing_retains_redemption_identity", func(t *testing.T) {
		type redemptionRequest struct {
			RequestID string `json:"request_id"`
			Code      string `json:"code"`
		}
		requests := make(chan redemptionRequest, 4)
		server := httptest.NewServer(http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
			var body redemptionRequest
			if request.URL.Path != "/v1/codes/redeem" || json.NewDecoder(request.Body).Decode(&body) != nil {
				t.Error("selected pairing changed the redemption request")
			}
			requests <- body
			response.WriteHeader(http.StatusServiceUnavailable)
		}))
		defer server.Close()
		link := newFixtureLink(t)
		var claim PairingClaim
		selection := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://selected.example"}}}
		var previous string
		for index, pin := range []string{"001234", "001234", "001234", "654321"} {
			var err error
			if index == 0 {
				_, err = link.PairPIN(t.Context(), " "+server.URL+"/ ", pin, &claim)
			} else {
				_, err = link.PairPINOnNetwork(t.Context(), server.URL, pin, selection, &claim)
				selection = protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://other.example"}}}
			}
			if err == nil || faultValue(err).Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("redemption failure: %v", err)
			}
			request := <-requests
			if request.Code != pin || !validPairingRequestID(request.RequestID) {
				t.Fatal("redemption did not retain the exact PIN and a valid request identity")
			}
			if index == 1 || index == 2 {
				if request.RequestID != previous {
					t.Fatal("selected pairing replaced the canonical origin and PIN request identity")
				}
			} else if index == 3 && request.RequestID == previous {
				t.Fatal("changed PIN reused a previous redemption request identity")
			}
			previous = request.RequestID
		}
		if _, err := link.PairPINOnNetwork(t.Context(), server.URL, "001234", protocol.NetworkSelection{Kind: "custom"}, &claim); faultValue(err).Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("invalid network selection reached redemption: %v", err)
		}
		select {
		case <-requests:
			t.Fatal("invalid network selection redeemed the PIN")
		default:
		}
	})
	for _, test := range []struct {
		name    string
		status  int
		body    string
		length  int64
		failure error
		code    protocol.ErrorCode
	}{
		{"valid", 201, `{"code":"001234"}`, -1, nil, ""},
		{"unknown_field", 201, `{"code":"001234","secret":true}`, -1, nil, protocol.ErrorCodeInvalidRequest},
		{"body_bound", 201, strings.Repeat(" ", maxPairingResponse+1), -1, nil, protocol.ErrorCodeInvalidRequest},
		{"declared_bound", 201, `{}`, maxPairingResponse + 1, nil, protocol.ErrorCodeInvalidRequest},
		{"expired", 410, `{}`, -1, nil, protocol.ErrorCodeNotFound},
		{"conflict", 409, `{}`, -1, nil, protocol.ErrorCodeConflict},
		{"limited", 429, `{}`, -1, nil, protocol.ErrorCodeBusy},
		{"redirect", 302, `{}`, -1, nil, protocol.ErrorCodeUnavailable},
		{"lost_response", 0, "", -1, errors.New("private transport detail"), protocol.ErrorCodeUnavailable},
	} {
		t.Run(test.name, func(t *testing.T) {
			relay, err := newPairingRelay("https://link.example", clock)
			if err != nil {
				t.Fatal(err)
			}
			relay.client.Timeout = 0
			calls := 0
			relay.client.Transport = pairingRoundTripper(func(request *http.Request) (*http.Response, error) {
				calls++
				if request.GetBody != nil || request.Method != http.MethodPost || request.URL.Path != "/v1/codes" {
					t.Error("pairing request permits replay or changed its endpoint")
				}
				if test.failure != nil {
					return nil, test.failure
				}
				return &http.Response{
					StatusCode: test.status, Body: io.NopCloser(strings.NewReader(test.body)),
					ContentLength: test.length, Header: http.Header{"Location": []string{"https://other.example/private"}},
					Request: request,
				}, nil
			})
			var result struct {
				Code string `json:"code"`
			}
			err = relay.post(t.Context(), "v1/codes", map[string]string{"request_id": first}, 201, &result, []string{"code"})
			if calls != 1 || test.code == "" && (err != nil || result.Code != "001234") ||
				test.code != "" && (err == nil || faultValue(err).Code != test.code) {
				t.Fatalf("bounded single-attempt exchange: %d, %#v, %v", calls, result, err)
			}
			if err != nil && strings.Contains(err.Error(), "private") {
				t.Fatal("pairing failure exposed transport details")
			}
		})
	}
	t.Run("publication_clamps_invitation", func(t *testing.T) {
		relay, err := newPairingRelay("https://link.example", clock)
		if err != nil {
			t.Fatal(err)
		}
		relay.client.Timeout = 0
		relay.client.Transport = pairingRoundTripper(func(request *http.Request) (*http.Response, error) {
			var body map[string]json.RawMessage
			if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
				t.Error(err)
			}
			var captured string
			if json.Unmarshal(body["request_id"], &captured) != nil || captured != first {
				t.Error("publication replaced its retained request identity")
			}
			return &http.Response{StatusCode: 201, ContentLength: -1,
				Body: io.NopCloser(strings.NewReader(`{"code":"001234","expires_at_ms":220000,"cancel_token":"` + strings.Repeat("a", 64) + `"}`))}, nil
		})
		invitation := &Invitation{ticket: "isolated private ticket", expiresAtMS: 160000}
		code, err := relay.publish(t.Context(), invitation, first)
		if err != nil || code.expiresAtMS != invitation.ExpiresAtMS() || code.code != "001234" {
			t.Fatalf("invitation publication: %#v, %v", code, err)
		}
	})
	t.Run("share_terminal_state_survives_close", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		session := &ShareSession{
			ctx: ctx, cancel: cancel, done: make(chan struct{}), changed: make(chan struct{}),
			state: ShareState{Kind: "preparing"}, version: 1,
		}
		if state, err := session.Next(t.Context()); err != nil || state.Kind != "preparing" {
			t.Fatalf("initial share state: %#v, %v", state, err)
		}
		session.publish(ShareState{Kind: "ready", Code: "001234", ExpiresAtMS: 160000})
		if state, err := session.Next(t.Context()); err != nil || state.Kind != "ready" || state.Code != "001234" {
			t.Fatalf("ready share state: %#v, %v", state, err)
		}
		session.publish(ShareState{Kind: "paired"})
		close(session.done)
		if state, err := session.Next(t.Context()); err != nil || state.Kind != "paired" {
			t.Fatalf("terminal share state: %#v, %v", state, err)
		}
		if _, err := session.Next(t.Context()); !errors.Is(err, io.EOF) {
			t.Fatalf("closed share stream: %v", err)
		}
	})
}
