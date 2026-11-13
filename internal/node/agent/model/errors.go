package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"slices"
	"sort"
	"strings"

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
	Kind    FailureKind
	Message string
	status  *uint16
	// Exhausted marks only Run's terminal recovery outcome. Kind still names
	// the provider failure, including when a summary records why it failed.
	Exhausted bool
	// WaitExhausted distinguishes the cumulative wait budget from the attempt limit.
	WaitExhausted bool
	retryAfter    []string
	retryAfterMs  *uint64
	overloaded    bool
}

func (f *Failure) Error() string { return f.Message }
func (f *Failure) Retryable() bool {
	return f.Kind == RateLimited || f.Kind == Network || f.Kind == Unavailable
}

// httpResponseFailure carries only retry advice across a transport's decoding
// failures. Keeping the cause preserves SDK error inspection without retaining
// unrelated response headers in a Failure.
type httpResponseFailure struct {
	cause      error
	status     *uint16
	retryAfter []string
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
	failure := &httpResponseFailure{cause: err, retryAfter: retryAfterValues(headers)}
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

func (f *Failure) retainResponseHints(err error) {
	var response *httpResponseFailure
	if errors.As(err, &response) {
		f.status = response.status
		f.retryAfter = slices.Clone(response.retryAfter)
	}
	f.overloaded = f.Retryable() && (f.overloaded || f.Kind == RateLimited || f.status != nil && *f.status == 529)
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
			if strings.EqualFold(name, "Retry-After") {
				headers[name] = []string{value}
			}
		}
		f.retryAfter = retryAfterValues(headers)
		if status > 0 && status <= 65535 {
			v := uint16(status)
			f.status = &v
		}
		var body map[string]any
		json.Unmarshal(provider.ResponseBody, &body)
		code := provider.ErrorType
		overloaded := overloadedCode(code)
		foundCode := false
		for _, path := range [][]string{{"error", "code"}, {"error", "type"}, {"response", "error", "code"}, {"code"}, {"type"}} {
			if value := jsonString(body, path...); value != "" {
				overloaded = overloaded || overloadedCode(value)
				if !foundCode {
					code, foundCode = value, true
				}
			}
		}
		message := ""
		for _, path := range [][]string{{"error", "message"}, {"response", "error", "message"}, {"message"}, {"detail"}} {
			if value := jsonString(body, path...); value != "" {
				message = value
				break
			}
		}
		if len(provider.ResponseBody) > 0 {
			if message == "" {
				message = "Model request failed"
			}
			if status > 0 {
				message = fmt.Sprintf("%s (HTTP %d)", message, status)
			} else if code != "" {
				message = code + ": " + message
			}
			f.Message = message
		}
		lower := strings.ToLower(string(provider.ResponseBody))
		switch {
		case status >= 300 && status <= 399:
			f.Kind = InvalidRequest
		case provider.ContextTooLargeErr || containsCode(code, "context_length_exceeded", "context_window_exceeded", "prompt_too_long", "request_too_large") || strings.Contains(lower, "maximum context length"):
			f.Kind = ContextOverflow
		case status == 401 || status == 403 || provider.AuthError || containsCode(code, "invalid_api_key", "authentication_error", "invalid_authentication", "unauthorized", "permission_denied"):
			f.Kind = Authentication
		case containsCode(code, "insufficient_quota", "credits_exhausted", "billing_not_active", "quota_exceeded", "billing_hard_limit_reached"):
			f.Kind = Quota
		case status == 429 || containsCode(code, "rate_limit_exceeded", "rate_limit_error", "too_many_requests"):
			f.Kind = RateLimited
		case status == 408 || status >= 500 && status <= 599 || overloaded || containsCode(code, "upstream_bad_gateway", "upstream_unavailable", "upstream_timeout", "server_error", "internal_error", "internal_server_error", "bad_gateway", "gateway_timeout", "service_unavailable", "service_unavailable_error"):
			f.Kind = Unavailable
		case errors.Is(provider.Cause, context.Canceled):
			f.Kind = Cancelled
		case errors.Is(provider.Cause, io.ErrUnexpectedEOF) || errors.As(provider.Cause, &malformed):
			f.Kind = InvalidResponse
		case errors.As(provider.Cause, &network) || errors.Is(provider.Cause, context.DeadlineExceeded):
			f.Kind = Network
		case status >= 400 && status <= 499:
			f.Kind = InvalidRequest
		default:
			f.Kind = InvalidResponse
		}
		f.overloaded = overloaded
	case errors.Is(err, io.ErrUnexpectedEOF) || errors.As(err, &malformed):
		f.Kind = InvalidResponse
	case errors.As(err, &network) || errors.Is(err, context.DeadlineExceeded):
		f.Kind = Network
	}
	if errors.Is(err, io.ErrUnexpectedEOF) {
		f.Message = "Model stream ended before a completion event"
	}
	if errors.Is(err, stallError{}) {
		f.Kind, f.Message = Network, "Model stream sent no data for 600 seconds"
	}
	f.retainResponseHints(err)
	f.Message = safeDiagnostic(f.Message, secrets)
	return f
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
		if failure.WaitExhausted {
			reason = protocol.ConversationRecoveryStopWaitExhausted
		}
		return &protocol.Fault{Code: code, Message: failure.Message, Details: &protocol.FaultDetails{Kind: "model_recovery", Data: protocol.FaultDetailsModelRecoveryPayload{Reason: reason}}}
	}
	reason := protocol.ModelFailureRequest
	switch failure.Kind {
	case Authentication:
		reason = protocol.ModelFailureUnauthorized
	case RateLimited, Quota:
		reason = protocol.ModelFailureRateLimited
	}
	// The code and reason are Rust's. An overflow is stated beside them, so
	// that a reader of the run can tell it from an outage.
	return &protocol.Fault{Code: code, Message: failure.Message, Details: &protocol.FaultDetails{Kind: "model_request", Data: protocol.FaultDetailsModelRequestPayload{Reason: reason, ContextOverflow: failure.Kind == ContextOverflow}}}
}
