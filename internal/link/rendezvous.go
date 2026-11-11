package link

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/tls"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const DefaultPairingService = "https://sailry.link.dux.plus"
const maxPairingResponse = 12 * 1024

type PairingClaim struct {
	mu      sync.Mutex
	origin  string
	pin     string
	request string
}

func (*PairingClaim) String() string   { return "PairingClaim([REDACTED])" }
func (*PairingClaim) GoString() string { return "PairingClaim([REDACTED])" }

type pairingRelay struct {
	base   *url.URL
	client *http.Client
	now    func() time.Time
}

type pairingCode struct {
	code        string
	expiresAtMS uint64
	cancelToken string
}

func newPairingRelay(origin string, now func() time.Time) (*pairingRelay, error) {
	if origin == "" {
		origin = DefaultPairingService
	}
	base, err := url.Parse(origin)
	if err != nil {
		return nil, invalidPairingExchange()
	}
	ip := net.ParseIP(base.Hostname())
	loopback := ip != nil && ip.IsLoopback()
	if !(base.Scheme == "https" || base.Scheme == "http" && loopback) || base.Hostname() == "" ||
		urlHasCredentials(base) || base.RawQuery != "" || base.ForceQuery || base.Fragment != "" ||
		strings.Contains(origin, "#") || base.Opaque != "" || base.Path != "" && base.Path != "/" {
		return nil, invalidPairingExchange()
	}
	if err := normalizeURLHost(base); err != nil {
		return nil, invalidPairingExchange()
	}
	base.Path = "/"
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.DisableKeepAlives = true
	transport.ForceAttemptHTTP2 = false
	transport.TLSNextProto = map[string]func(string, *tls.Conn) http.RoundTripper{}
	if loopback {
		transport.Proxy = nil
	}
	client := &http.Client{
		Transport: transport, Timeout: ioTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	return &pairingRelay{base: base, client: client, now: now}, nil
}

func (link *Link) PairPIN(ctx context.Context, origin, pin string, claim *PairingClaim) (EndpointAddress, error) {
	return link.pairPINSelected(ctx, origin, pin, nil, claim)
}

func (link *Link) PairPINOnNetwork(ctx context.Context, origin, pin string, selection protocol.NetworkSelection, claim *PairingClaim) (EndpointAddress, error) {
	relays, err := NetworkRelayURLs(ctx, selection)
	if err != nil {
		return EndpointAddress{}, err
	}
	return link.pairPINSelected(ctx, origin, pin, relays, claim)
}

func (link *Link) pairPINSelected(ctx context.Context, origin, pin string, relays []string, claim *PairingClaim) (EndpointAddress, error) {
	if claim == nil || !validPairingPIN(pin) {
		return EndpointAddress{}, invalidPairingExchange()
	}
	relay, err := newPairingRelay(strings.TrimSpace(origin), link.now)
	if err != nil {
		return EndpointAddress{}, err
	}
	defer relay.client.CloseIdleConnections()
	scoped, cancel := link.operation(ctx, false)
	defer cancel()
	request, err := claim.requestID(relay.base.String(), pin, relay.now())
	if err != nil {
		return EndpointAddress{}, err
	}
	var redemption struct {
		Ticket      string `json:"ticket"`
		ExpiresAtMS uint64 `json:"expires_at_ms"`
	}
	if err := relay.post(scoped, "v1/codes/redeem", struct {
		RequestID string `json:"request_id"`
		Code      string `json:"code"`
	}{request, pin}, http.StatusOK, &redemption, []string{"ticket", "expires_at_ms"}); err != nil {
		return EndpointAddress{}, err
	}
	// The relay released the ticket and the sharer decides whether it is
	// still valid; this clock is not compared with theirs.
	return link.pairSelected(scoped, redemption.Ticket, relays)
}

func (claim *PairingClaim) requestID(origin, pin string, now time.Time) (string, error) {
	claim.mu.Lock()
	defer claim.mu.Unlock()
	if claim.request == "" || claim.origin != origin || claim.pin != pin {
		request, err := newPairingRequestID(now)
		if err != nil {
			return "", err
		}
		claim.origin, claim.pin, claim.request = origin, pin, request
	}
	return claim.request, nil
}

func (relay *pairingRelay) publish(ctx context.Context, invitation *Invitation, request string) (pairingCode, error) {
	now, err := pairingNowMS(relay.now())
	if err != nil {
		return pairingCode{}, err
	}
	if invitation.ExpiresAtMS() <= now {
		return pairingCode{}, pairingCodeExpired()
	}
	var code struct {
		Code        string `json:"code"`
		ExpiresAtMS uint64 `json:"expires_at_ms"`
		CancelToken string `json:"cancel_token"`
	}
	if err := relay.post(ctx, "v1/codes", struct {
		RequestID   string `json:"request_id"`
		Ticket      string `json:"ticket"`
		ExpiresAtMS uint64 `json:"expires_at_ms"`
	}{request, invitation.Ticket(), invitation.ExpiresAtMS()}, http.StatusCreated, &code, []string{"code", "expires_at_ms", "cancel_token"}); err != nil {
		return pairingCode{}, err
	}
	if !validPairingPIN(code.Code) || !validPairingRequestID(code.CancelToken) {
		return pairingCode{}, invalidPairingExchange()
	}
	code.ExpiresAtMS = min(code.ExpiresAtMS, invitation.ExpiresAtMS())
	now, err = pairingNowMS(relay.now())
	if err != nil {
		return pairingCode{}, err
	}
	if code.ExpiresAtMS <= now {
		return pairingCode{}, pairingCodeExpired()
	}
	return pairingCode{code.Code, code.ExpiresAtMS, code.CancelToken}, nil
}

func (relay *pairingRelay) cancel(ctx context.Context, code pairingCode) error {
	return relay.post(ctx, "v1/codes/cancel", struct {
		Code        string `json:"code"`
		CancelToken string `json:"cancel_token"`
	}{code.code, code.cancelToken}, http.StatusNoContent, nil, nil)
}

func (relay *pairingRelay) post(ctx context.Context, path string, body any, expected int, result any, fields []string) error {
	encoded, err := json.Marshal(body)
	if err != nil {
		return invalidPairingExchange()
	}
	endpoint := relay.base.ResolveReference(&url.URL{Path: path})
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint.String(), io.NopCloser(bytes.NewReader(encoded)))
	if err != nil {
		return invalidPairingExchange()
	}
	request.ContentLength = int64(len(encoded))
	request.Header.Set("Content-Type", "application/json")
	response, err := relay.client.Do(request)
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "pairing service request failed"}
	}
	defer response.Body.Close()
	if response.StatusCode != expected {
		switch response.StatusCode {
		case http.StatusNotFound, http.StatusGone:
			return pairingCodeExpired()
		case http.StatusConflict:
			return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "pairing request conflict"}
		case http.StatusTooManyRequests:
			return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "pairing service rate limit"}
		default:
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: fmt.Sprintf("pairing service returned HTTP %d", response.StatusCode)}
		}
	}
	if expected == http.StatusNoContent {
		return nil
	}
	if response.ContentLength > maxPairingResponse {
		return invalidPairingExchange()
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, maxPairingResponse+1))
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "pairing service response failed"}
	}
	if len(data) > maxPairingResponse || protocol.DecodeObject(data, result, fields, true) != nil {
		return invalidPairingExchange()
	}
	return nil
}

func newPairingRequestID(now time.Time) (string, error) {
	milliseconds, err := pairingNowMS(now)
	if err != nil {
		return "", err
	}
	var random [26]byte
	if _, err := rand.Read(random[:]); err != nil {
		return "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "pairing exchange is unavailable"}
	}
	return fmt.Sprintf("%012x%s", milliseconds, hex.EncodeToString(random[:])), nil
}

func pairingNowMS(now time.Time) (uint64, error) {
	if now.UnixMilli() < 0 {
		return 0, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "pairing exchange is unavailable"}
	}
	return uint64(now.UnixMilli()), nil
}

func validPairingPIN(pin string) bool {
	if len(pin) != 6 {
		return false
	}
	for _, character := range []byte(pin) {
		if character < '0' || character > '9' {
			return false
		}
	}
	return true
}

func validPairingRequestID(value string) bool {
	if len(value) != 64 {
		return false
	}
	for _, character := range []byte(value) {
		if !(character >= '0' && character <= '9' || character >= 'a' && character <= 'f') {
			return false
		}
	}
	return true
}

func invalidPairingExchange() error {
	return invalidLink("invalid pairing exchange")
}

func pairingCodeExpired() error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "pairing code is unavailable"}
}
