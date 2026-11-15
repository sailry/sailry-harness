package link

import (
	"bytes"
	"context"
	"crypto/x509"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type pairingRoundTripper func(*http.Request) (*http.Response, error)

func (transport pairingRoundTripper) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

type pairingInterruptedReader struct {
	io.Reader
	failure error
}

func (reader pairingInterruptedReader) Read(data []byte) (int, error) {
	count, err := reader.Reader.Read(data)
	if err == io.EOF {
		err = reader.failure
	}
	return count, err
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
			_, _ = io.WriteString(response, `{"error":"capacity"}`)
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

func TestPairingPINRedemptionRecovery(t *testing.T) {
	t.Parallel()
	const recovered = `{"ticket":"opaque-ticket","expires_at_ms":160000}`
	for _, test := range []struct {
		name        string
		status      int
		body        string
		failure     error
		readFailure error
		oversized   bool
		repeat      bool
		retry       bool
		code        protocol.ErrorCode
	}{
		{name: "first_attempt", status: 200, body: recovered},
		{name: "bad_gateway", status: 502, retry: true},
		{name: "unavailable", status: 503, body: `{"error":"unavailable"}`, retry: true},
		{name: "gateway_timeout", status: 504, body: `{"error":"unavailable"}`, retry: true},
		{name: "lost_response", failure: io.EOF, retry: true},
		{name: "connection_reset", failure: &net.OpError{Op: "read", Net: "tcp", Err: syscall.ECONNRESET}, retry: true},
		{name: "connection_refused", failure: &net.OpError{Op: "dial", Net: "tcp", Err: syscall.ECONNREFUSED}, retry: true},
		{name: "timeout", failure: &net.OpError{Op: "read", Net: "tcp", Err: context.DeadlineExceeded}, retry: true},
		{name: "temporary_dns", failure: &net.DNSError{IsTemporary: true}, retry: true},
		{name: "interrupted_body", status: 200, body: `{"ticket":"opaque`, readFailure: io.ErrUnexpectedEOF, retry: true},
		{name: "stops_after_one_retry", status: 503, body: `{"error":"unavailable"}`, repeat: true, retry: true, code: protocol.ErrorCodeUnavailable},
		{name: "capacity", status: 503, body: `{"error":"capacity"}`, code: protocol.ErrorCodeUnavailable},
		{name: "invalid_body", status: 503, body: `{"error":"invalid_body"}`, code: protocol.ErrorCodeUnavailable},
		{name: "malformed_error", status: 503, body: `{`, code: protocol.ErrorCodeUnavailable},
		{name: "unknown_error_field", status: 503, body: `{"error":"unavailable","extra":true}`, code: protocol.ErrorCodeUnavailable},
		{name: "oversized_error", status: 503, body: strings.Repeat(" ", maxPairingResponse+1), code: protocol.ErrorCodeUnavailable},
		{name: "declared_oversized_error", status: 503, oversized: true, code: protocol.ErrorCodeUnavailable},
		{name: "interrupted_error_body", status: 503, body: `{"error":`, readFailure: io.ErrUnexpectedEOF, code: protocol.ErrorCodeUnavailable},
		{name: "invalid_ticket_response", status: 200, body: `{`, code: protocol.ErrorCodeInvalidRequest},
		{name: "unknown_ticket_field", status: 200, body: `{"ticket":"opaque","expires_at_ms":160000,"extra":true}`, code: protocol.ErrorCodeInvalidRequest},
		{name: "oversized_ticket", status: 200, body: strings.Repeat(" ", maxPairingResponse+1), code: protocol.ErrorCodeInvalidRequest},
		{name: "declared_oversized_ticket", status: 200, oversized: true, code: protocol.ErrorCodeInvalidRequest},
		{name: "unauthorized", status: 401, code: protocol.ErrorCodeUnavailable},
		{name: "forbidden", status: 403, code: protocol.ErrorCodeUnavailable},
		{name: "expired", status: 404, code: protocol.ErrorCodeNotFound},
		{name: "gone", status: 410, code: protocol.ErrorCodeNotFound},
		{name: "conflict", status: 409, code: protocol.ErrorCodeConflict},
		{name: "rate_limit", status: 429, code: protocol.ErrorCodeBusy},
		{name: "internal_error", status: 500, code: protocol.ErrorCodeUnavailable},
		{name: "redirect", status: 302, code: protocol.ErrorCodeUnavailable},
		{name: "permanent_dns", failure: &net.DNSError{IsNotFound: true}, code: protocol.ErrorCodeUnavailable},
		{name: "certificate", failure: x509.UnknownAuthorityError{}, code: protocol.ErrorCodeUnavailable},
		{name: "cancelled_transport", failure: context.Canceled, code: protocol.ErrorCodeUnavailable},
		{name: "other_transport", failure: errors.New("private transport detail"), code: protocol.ErrorCodeUnavailable},
		{name: "other_response", status: 200, readFailure: errors.New("private response detail"), code: protocol.ErrorCodeUnavailable},
	} {
		t.Run(test.name, func(t *testing.T) {
			link := newFixtureLink(t)
			relay, err := newPairingRelay("https://link.example", link.now)
			if err != nil {
				t.Fatal(err)
			}
			relay.client.Timeout = 0
			waits := 0
			link.timeout = func(ctx context.Context, duration time.Duration) (context.Context, context.CancelFunc) {
				waits++
				if duration != 500*time.Millisecond {
					t.Fatalf("redemption retry wait: %s", duration)
				}
				waiting, cancel := context.WithCancel(ctx)
				cancel()
				return waiting, cancel
			}
			var claim PairingClaim
			requestID, err := claim.requestID(relay.base.String(), "001234", link.now())
			if err != nil {
				t.Fatal(err)
			}
			calls := 0
			var original []byte
			relay.client.Transport = pairingRoundTripper(func(request *http.Request) (*http.Response, error) {
				calls++
				if request.GetBody != nil || request.URL.Path != "/v1/codes/redeem" || request.Method != http.MethodPost {
					t.Fatal("redemption changed its endpoint or enabled automatic transport replay")
				}
				body, err := io.ReadAll(request.Body)
				if err != nil {
					t.Fatal(err)
				}
				if calls == 1 {
					original = body
				} else if !bytes.Equal(original, body) {
					t.Fatal("redemption retry changed its original body")
				}
				var submitted struct {
					RequestID string `json:"request_id"`
					Code      string `json:"code"`
				}
				if err := json.Unmarshal(body, &submitted); err != nil || submitted.RequestID != requestID || submitted.Code != "001234" {
					t.Fatal("redemption lost its PIN or request identity")
				}
				status, value := 200, recovered
				var readFailure error
				length := int64(-1)
				if calls == 1 || test.repeat {
					if test.failure != nil {
						return nil, test.failure
					}
					status, value, readFailure = test.status, test.body, test.readFailure
					if test.oversized {
						length = maxPairingResponse + 1
					}
				}
				var reader io.Reader = strings.NewReader(value)
				if readFailure != nil {
					reader = pairingInterruptedReader{Reader: reader, failure: readFailure}
				}
				return &http.Response{
					StatusCode: status, ContentLength: length, Body: io.NopCloser(reader), Request: request,
					Header: http.Header{"Retry-After": []string{"60"}},
				}, nil
			})
			ticket, err := link.redeemPIN(t.Context(), relay, requestID, "001234")
			attempts, expectedWaits := 1, 0
			if test.retry {
				attempts, expectedWaits = 2, 1
			}
			if calls != attempts || waits != expectedWaits {
				t.Fatalf("redemption attempts=%d waits=%d, want %d and %d", calls, waits, attempts, expectedWaits)
			}
			if test.code == "" {
				if err != nil || ticket != "opaque-ticket" {
					t.Fatalf("recovered redemption: %q, %v", ticket, err)
				}
			} else if err == nil || faultValue(err).Code != test.code {
				t.Fatalf("redemption fault: %v, want %s", err, test.code)
			}
			if err != nil && strings.Contains(err.Error(), "private") {
				t.Fatal("redemption exposed transport details")
			}
		})
	}
}

func TestPairingPINRedemptionCancellation(t *testing.T) {
	t.Parallel()
	for _, stage := range []string{"before_request", "during_request", "during_wait"} {
		t.Run(stage, func(t *testing.T) {
			link := newFixtureLink(t)
			relay, err := newPairingRelay("https://link.example", link.now)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			calls, waits := 0, 0
			link.timeout = func(ctx context.Context, duration time.Duration) (context.Context, context.CancelFunc) {
				waits++
				if duration != 500*time.Millisecond {
					t.Fatalf("redemption retry wait: %s", duration)
				}
				cancel()
				return context.WithCancel(ctx)
			}
			relay.client.Transport = pairingRoundTripper(func(request *http.Request) (*http.Response, error) {
				calls++
				if stage == "during_request" {
					cancel()
					return nil, context.Canceled
				}
				return &http.Response{StatusCode: 503, ContentLength: -1,
					Body: io.NopCloser(strings.NewReader(`{"error":"unavailable"}`)), Request: request}, nil
			})
			if stage == "before_request" {
				cancel()
			}
			_, err = link.redeemPIN(ctx, relay, strings.Repeat("a", 64), "001234")
			if !errors.Is(err, context.Canceled) {
				t.Fatalf("cancelled redemption: %v", err)
			}
			expectedCalls, expectedWaits := 1, 0
			if stage == "before_request" {
				expectedCalls = 0
			} else if stage == "during_wait" {
				expectedWaits = 1
			}
			if calls != expectedCalls || waits != expectedWaits {
				t.Fatalf("cancelled redemption attempts=%d waits=%d, want %d and %d", calls, waits, expectedCalls, expectedWaits)
			}
		})
	}
}
