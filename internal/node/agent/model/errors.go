package model

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type FailureKind string

const (
	NotConfigured   FailureKind = "not_configured"
	Authentication  FailureKind = "authentication"
	ContextOverflow FailureKind = "context_overflow"
	RateLimited     FailureKind = "rate_limited"
	Quota           FailureKind = "quota"
	Network         FailureKind = "network"
	Unavailable     FailureKind = "unavailable"
	InvalidRequest  FailureKind = "invalid_request"
	InvalidResponse FailureKind = "invalid_response"
	Unsupported     FailureKind = "unsupported"
	Cancelled       FailureKind = "cancelled"
	Other           FailureKind = "other"
)

type Failure struct {
	Kind            FailureKind
	Message         string
	status          *uint16
	requestObserved bool
	// Exhausted marks only Run's terminal recovery outcome. Kind still names
	// the provider failure, including when a summary records why it failed.
	Exhausted bool
	// WaitExhausted distinguishes the cumulative wait budget from the attempt limit.
	WaitExhausted  bool
	retryAfter     []string
	retryAfterMs   *uint64
	overloaded     bool
	tcpDialTimeout bool
	streamIdle     time.Duration
	// A transport EOF can recover; other invalid responses remain permanent.
	truncated bool
	// forbidden marks an Authentication failure the provider refused with
	// HTTP 403 rather than a rejected key. Relay gateways that pool upstream
	// accounts answer 403 while they rotate to another account, so it uses
	// the shared retry policy; a rejected key only gets the HTTP resend.
	forbidden bool
	// concurrencyLimited marks an upstream rejection caused by too many
	// concurrent in-flight requests for the same credential.
	concurrencyLimited bool
}

func (f *Failure) Error() string            { return f.Message }
func (f *Failure) RequestObserved() bool    { return f.requestObserved }
func (f *Failure) StreamStalled() bool      { return f != nil && f.streamIdle > 0 }
func (f *Failure) ConcurrencyLimited() bool { return f != nil && f.concurrencyLimited }
func (f *Failure) Retryable() bool {
	return f.Kind == RateLimited || f.Kind == Quota || f.Kind == Network || f.Kind == Unavailable || f.truncated || f.Kind == Authentication && f.forbidden
}

// loadRetryable is a retry that waits out provider load. An account refused
// for its balance or permission is retried too, but it is not overload.
func (f *Failure) loadRetryable() bool {
	return f.Retryable() && f.Kind != Quota && f.Kind != Authentication
}

// nativeFallback is a retryable failure that Sailry's own summary may not
// meet: a provider error or outage rather than load, a balance or an account.
func (f *Failure) nativeFallback() bool {
	return f.loadRetryable() && !f.overloaded
}

// httpResponseFailure carries only retry advice across a transport's decoding
// failures. Keeping the cause preserves SDK error inspection without retaining
// unrelated response headers in a Failure.
type httpResponseFailure struct {
	cause        error
	status       *uint16
	retryAfter   []string
	retryAfterMs *uint64
}

func (f *httpResponseFailure) Error() string { return f.cause.Error() }
func (f *httpResponseFailure) Unwrap() error { return f.cause }

func withResponseHints(err error, status int, headers http.Header) error {
	if err == nil {
		return nil
	}
	var retained *httpResponseFailure
	if errors.As(err, &retained) {
		return err
	}
	failure := &httpResponseFailure{cause: err, retryAfter: retryAfterValues(headers), retryAfterMs: retryAfterMsValue(headers)}
	if status > 0 && status <= 65535 {
		value := uint16(status)
		failure.status = &value
	}
	return failure
}

func retryAfterValues(headers http.Header) []string {
	var names []string
	for name := range headers {
		if strings.EqualFold(name, "Retry-After") {
			names = append(names, name)
		}
	}
	sort.Strings(names)
	var values []string
	for _, name := range names {
		values = append(values, headers[name]...)
	}
	return values
}

func retryAfterMsValue(headers http.Header) *uint64 {
	var best *uint64
	for name, values := range headers {
		if !strings.EqualFold(name, "Retry-After-Ms") {
			continue
		}
		for _, raw := range values {
			value := strings.TrimSpace(raw)
			millis, err := strconv.ParseUint(value, 10, 64)
			if err != nil {
				if errors.Is(err, strconv.ErrRange) {
					maxVal := ^uint64(0)
					best = &maxVal
				}
				continue
			}
			if best == nil || millis > *best {
				v := millis
				best = &v
			}
		}
	}
	return best
}

func (f *Failure) retainResponseHints(err error) {
	var response *httpResponseFailure
	if errors.As(err, &response) {
		f.status = response.status
		f.retryAfter = slices.Clone(response.retryAfter)
		if response.retryAfterMs != nil {
			v := *response.retryAfterMs
			if f.retryAfterMs == nil || v > *f.retryAfterMs {
				f.retryAfterMs = &v
			}
		}
	}
	if f.status != nil || len(f.retryAfter) > 0 || f.retryAfterMs != nil {
		f.tcpDialTimeout = false
	}
	f.overloaded = f.loadRetryable() && (f.overloaded || f.Kind == RateLimited || f.status != nil && *f.status == 529)
}

func overloadedCode(code string) bool {
	return containsCode(code, "server_is_overloaded", "overloaded_error", "server_overloaded", "overloaded", "model_overloaded", "server_busy", "model_busy", "busy")
}

func Classify(err error, secrets ...protocol.Secret) *Failure {
	if err == nil {
		return nil
	}
	var existing *Failure
	if errors.As(err, &existing) {
		copy := *existing
		copy.retryAfter = slices.Clone(existing.retryAfter)
		if existing.retryAfterMs != nil {
			value := *existing.retryAfterMs
			copy.retryAfterMs = &value
		}
		copy.retainResponseHints(err)
		copy.Message = safeDiagnostic(copy.Message, secrets)
		return &copy
	}
	f := &Failure{Kind: Other, Message: err.Error()}
	var provider *fantasy.ProviderError
	var network net.Error
	var malformed *json.SyntaxError
	switch {
	case errors.Is(err, context.Canceled):
		f.Kind = Cancelled
	case errors.As(err, &provider):
		status := provider.StatusCode
		headers := http.Header{}
		for name, value := range provider.ResponseHeaders {
			if strings.EqualFold(name, "Retry-After") || strings.EqualFold(name, "Retry-After-Ms") {
				headers[name] = []string{value}
			}
		}
		f.retryAfter = retryAfterValues(headers)
		f.retryAfterMs = retryAfterMsValue(headers)
		if status > 0 && status <= 65535 {
			v := uint16(status)
			f.status = &v
		}
		var body map[string]any
		json.Unmarshal(provider.ResponseBody, &body)
		code := provider.ErrorType
		overloaded := overloadedCode(code)
		overflow := provider.ContextTooLargeErr || contextOverflowCode(code)
		usageMissing := containsCode(code, "upstream_usage_missing")
		// A generic code can stand beside the specific type, so every code
		// the response names takes part in the classification; the first
		// one labels the message.
		codes := []string{provider.ErrorType}
		foundCode := false
		for _, path := range [][]string{{"error", "code"}, {"error", "type"}, {"response", "error", "code"}, {"response", "error", "type"}, {"code"}, {"type"}} {
			if value := jsonCode(body, path...); value != "" {
				overloaded = overloaded || overloadedCode(value)
				overflow = overflow || contextOverflowCode(value)
				usageMissing = usageMissing || containsCode(value, "upstream_usage_missing")
				codes = append(codes, value)
				if !foundCode {
					code, foundCode = value, true
				}
			}
		}
		invalidKey := anyCode(codes, invalidKeyCodes...)
		message := ""
		for _, path := range [][]string{{"error", "message"}, {"response", "error", "message"}, {"message"}, {"detail"}} {
			if value := jsonString(body, path...); value != "" {
				message = value
				break
			}
		}
		for _, path := range [][]string{{"error", "reason"}, {"response", "error", "reason"}, {"reason"}} {
			usageMissing = usageMissing || containsCode(jsonString(body, path...), "upstream_usage_missing")
		}
		usageMissing = (status == 0 || status >= 200 && status <= 299) && (usageMissing || strings.HasPrefix(message, `error: code=502 reason="UPSTREAM_USAGE_MISSING" message="`))
		if len(provider.ResponseBody) > 0 {
			if message == "" {
				message = "Model request failed"
			}
			if status > 0 {
				if status < 200 || status >= 300 {
					message = fmt.Sprintf("%s (HTTP %d)", message, status)
				}
			} else if code != "" {
				message = code + ": " + message
			}
			f.Message = message
		} else if cause := transportCause(provider.Cause); cause != "" && !strings.Contains(f.Message, cause) {
			// Without a response body the transport's error is the only
			// account of why the stream broke.
			f.Message += ": " + cause
		}
		lower := strings.ToLower(string(provider.ResponseBody))
		concurrencyLimit := anyCode(codes, concurrencyLimitCodes...) ||
			concurrencyLimitMessage(message) || concurrencyLimitMessage(string(provider.ResponseBody))
		switch {
		case status >= 300 && status <= 399:
			f.Kind = InvalidRequest
		case overflow || strings.Contains(lower, "maximum context length") ||
			strings.Contains(strings.ToLower(message), "input exceeds the context window"):
			f.Kind = ContextOverflow
		case !invalidKey && (status == 402 || anyCode(codes, "insufficient_quota", "credits_exhausted", "billing_not_active", "quota_exceeded", "billing_hard_limit_reached", "insufficient_balance", "insufficient_credits", "payment_required") ||
			strings.Contains(lower, "all available accounts exhausted")):
			// A declared balance code is checked before the authentication
			// rule, since gateways send it with 403. A refusal stated only in
			// prose stays a 403 and is shown with the provider's own message.
			f.Kind = Quota
		case status == 401 || status == 403 || provider.AuthError || invalidKey || anyCode(codes, "authentication_error", "unauthorized", "permission_denied"):
			f.Kind = Authentication
			// A 403 that names a credential failure is not an account rotation.
			f.forbidden = status == 403 && !invalidKey && !anyCode(codes, "authentication_error", "unauthorized", "AccessDeniedException")
		case status == 429 || anyCode(codes, rateLimitCodes...) || concurrencyLimit:
			f.Kind = RateLimited
			f.concurrencyLimited = concurrencyLimit
		// Bedrock reports stream exceptions by type after a 200 response.
		case status == 408 || status >= 500 && status <= 599 || overloaded || usageMissing || anyCode(codes, "upstream_bad_gateway", "upstream_unavailable", "upstream_timeout", "server_error", "internal_error", "internal_server_error", "bad_gateway", "gateway_timeout", "service_unavailable", "service_unavailable_error",
			"serviceUnavailableException", "internalServerException", "modelStreamErrorException", "modelTimeoutException"):
			f.Kind = Unavailable
		case errors.Is(provider.Cause, context.Canceled):
			f.Kind = Cancelled
		case status >= 400 && status <= 499:
			f.Kind = InvalidRequest
		case anyCode(codes, "unavailable", "upstream_stream_interrupted") || strings.EqualFold(strings.TrimSpace(message), "Upstream response stream was interrupted"):
			f.Kind = Unavailable
		case errors.Is(provider.Cause, io.ErrUnexpectedEOF) || errors.As(provider.Cause, &malformed):
			f.Kind = InvalidResponse
		case http2Interrupted(err) || errors.As(provider.Cause, &network) || errors.Is(provider.Cause, context.DeadlineExceeded):
			f.Kind = Network
		default:
			f.Kind = InvalidResponse
		}
		f.overloaded = overloaded
	case errors.Is(err, io.ErrUnexpectedEOF) || errors.As(err, &malformed):
		f.Kind = InvalidResponse
	case http2Interrupted(err) || errors.As(err, &network) || errors.Is(err, context.DeadlineExceeded):
		f.Kind = Network
	}
	if f.Kind == Network && (provider == nil || provider.StatusCode == 0 && len(provider.ResponseBody) == 0 && len(provider.ResponseHeaders) == 0) {
		f.tcpDialTimeout = isTCPDialTimeout(err)
	}
	if f.Kind == InvalidResponse && errors.Is(err, io.ErrUnexpectedEOF) {
		f.truncated = true
		f.Message = "Model stream ended before a completion event"
	}
	if stall, ok := errors.AsType[stallError](err); ok {
		f.streamIdle = stall.idle
		f.Kind, f.Message = Network, fmt.Sprintf("Model stream sent no data for %g seconds", stall.idle.Seconds())
	}
	f.retainResponseHints(err)
	f.Message = safeDiagnostic(f.Message, secrets)
	return f
}

var (
	concurrencyLimitCodes = []string{
		"concurrency_limit_exceeded",
		"concurrent_limit_exceeded",
		"too_many_concurrent_requests",
		"concurrent_requests_limit_exceeded",
		"concurrency_limit",
		"1302",
	}
	rateLimitCodes = []string{
		"rate_limit_exceeded",
		"rate_limit_error",
		"too_many_requests",
		"throttlingException",
		"resource_exhausted",
		"concurrency_limit_exceeded",
		"concurrent_limit_exceeded",
		"too_many_concurrent_requests",
		"concurrent_requests_limit_exceeded",
		"concurrency_limit",
		"1302",
	}
	invalidKeyCodes = []string{"invalid_api_key", "invalid_authentication"}
)

func concurrencyLimitMessage(text string) bool {
	if text == "" {
		return false
	}
	lower := strings.ToLower(text)
	if strings.Contains(lower, "concurrency limit") || strings.Contains(lower, "concurrent limit") ||
		strings.Contains(lower, "too many concurrent") || strings.Contains(lower, "max concurrency") {
		return true
	}
	if strings.Contains(lower, "concurrent request") && (strings.Contains(lower, "limit") || strings.Contains(lower, "exceeded") || strings.Contains(lower, "too many")) {
		return true
	}
	return strings.Contains(text, "并发") && (strings.Contains(text, "限制") || strings.Contains(text, "超限") || strings.Contains(text, "上限"))
}

// anyCode reports whether any of the response's codes is one of codes.
func anyCode(values []string, codes ...string) bool {
	return slices.ContainsFunc(values, func(value string) bool { return containsCode(value, codes...) })
}

func contextOverflowCode(code string) bool {
	return containsCode(code, "context_length_exceeded", "context_window_exceeded", "prompt_too_long", "request_too_large")
}

func isTCPDialTimeout(err error) bool {
	op, ok := errors.AsType[*net.OpError](err)
	if ok && op != nil && op.Op == "proxyconnect" && op.Net == "tcp" {
		op, ok = op.Err.(*net.OpError)
	}
	if !ok || op == nil || op.Op != "dial" || (op.Net != "tcp" && op.Net != "tcp4" && op.Net != "tcp6") {
		return false
	}
	// DNS failures from DialContext do not have a resolved TCP address.
	address, ok := op.Addr.(*net.TCPAddr)
	return ok && address != nil && op.Timeout()
}

func certificateVerificationFailure(err error, secrets ...protocol.Secret) *Failure {
	if certificate, ok := errors.AsType[*tls.CertificateVerificationError](err); ok && certificate != nil {
		return &Failure{Kind: InvalidRequest, Message: "model connection failed: " + transportCause(certificate, secrets...)}
	}
	return nil
}

// transportCause is a transport error's own text, without the request URL a
// url.Error repeats, on one line of at most 200 characters.
func transportCause(err error, secrets ...protocol.Secret) string {
	if err == nil {
		return ""
	}
	var request *url.Error
	if errors.As(err, &request) {
		err = request.Err
	}
	text := strings.Join(strings.Fields(safeDiagnostic(err.Error(), secrets)), " ")
	if runes := []rune(text); len(runes) > 200 {
		text = string(runes[:200]) + "…"
	}
	return text
}

func containsCode(value string, codes ...string) bool {
	for _, code := range codes {
		if strings.EqualFold(value, code) {
			return true
		}
	}
	return false
}
func jsonString(body map[string]any, path ...string) string {
	var value any = body
	for _, part := range path {
		object, ok := value.(map[string]any)
		if !ok {
			return ""
		}
		value = object[part]
	}
	result, _ := value.(string)
	return result
}

func jsonCode(body map[string]any, path ...string) string {
	var value any = body
	for _, part := range path {
		object, ok := value.(map[string]any)
		if !ok {
			return ""
		}
		value = object[part]
	}
	switch v := value.(type) {
	case string:
		return v
	case float64:
		if v == float64(int64(v)) {
			return strconv.FormatInt(int64(v), 10)
		}
	case json.Number:
		return v.String()
	}
	return ""
}
func safeDiagnostic(message string, secrets []protocol.Secret) string {
	values := make([]string, 0, len(secrets))
	for _, secret := range secrets {
		if value := secret.Expose(); value != "" {
			values = append(values, value)
		}
	}
	sort.Slice(values, func(i, j int) bool { return len(values[i]) > len(values[j]) })
	for _, value := range values {
		message = strings.ReplaceAll(message, value, "[redacted]")
	}
	return message
}
func Fault(err error) *protocol.Fault {
	var existing *protocol.Fault
	if errors.As(err, &existing) {
		copy := *existing
		return &copy
	}
	failure := Classify(err)
	if failure == nil {
		return nil
	}
	code := protocol.ErrorCodeUnavailable
	if failure.Kind == NotConfigured {
		code = protocol.ErrorCodeNotConfigured
	}
	if failure.Exhausted || failure.WaitExhausted {
		reason := protocol.ConversationRecoveryStopAttemptsExhausted
		switch {
		case failure.Kind == Quota:
			// A spent balance outlasted the retries: that, not the attempt
			// count, is what the user can act on.
			reason = protocol.ConversationRecoveryStopQuotaExceeded
		case failure.WaitExhausted:
			reason = protocol.ConversationRecoveryStopWaitExhausted
		}
		return &protocol.Fault{Code: code, Message: failure.Message, Details: &protocol.FaultDetails{Kind: "model_recovery", Data: protocol.FaultDetailsModelRecoveryPayload{Reason: reason}}}
	}
	reason := protocol.ModelFailureRequest
	switch failure.Kind {
	case Authentication:
		// A 403 is not known to be a credential failure: its title stays
		// generic and the provider's message says why.
		if !failure.forbidden {
			reason = protocol.ModelFailureUnauthorized
		}
	case RateLimited:
		reason = protocol.ModelFailureRateLimited
	case Quota:
		reason = protocol.ModelFailureQuota
	}
	// The code and reason are Rust's. An overflow is stated beside them, so
	// that a reader of the run can tell it from an outage.
	return &protocol.Fault{Code: code, Message: failure.Message, Details: &protocol.FaultDetails{Kind: "model_request", Data: protocol.FaultDetailsModelRequestPayload{Reason: reason, ContextOverflow: failure.Kind == ContextOverflow}}}
}
