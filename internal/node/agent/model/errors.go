package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
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
	Kind       FailureKind
	Message    string
	RetryAfter *time.Duration
	status     *uint16
}

func (f *Failure) Error() string { return f.Message }
func (f *Failure) Retryable() bool {
	return f.Kind == RateLimited || f.Kind == Network || f.Kind == Unavailable
}
func Classify(err error, secrets ...protocol.Secret) *Failure {
	if err == nil {
		return nil
	}
	var existing *Failure
	if errors.As(err, &existing) {
		copy := *existing
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
		if status > 0 && status <= 65535 {
			v := uint16(status)
			f.status = &v
		}
		var body map[string]any
		json.Unmarshal(provider.ResponseBody, &body)
		code := provider.ErrorType
		for _, path := range [][]string{{"error", "code"}, {"error", "type"}, {"response", "error", "code"}, {"code"}, {"type"}} {
			if value := jsonString(body, path...); value != "" {
				code = value
				break
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
		case status >= 500 && status <= 599 || containsCode(code, "upstream_bad_gateway", "upstream_unavailable", "upstream_timeout", "server_error", "internal_error", "internal_server_error", "bad_gateway", "gateway_timeout", "service_unavailable", "service_unavailable_error", "server_is_overloaded", "overloaded_error"):
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
		for key, value := range provider.ResponseHeaders {
			if strings.EqualFold(key, "Retry-After") {
				f.RetryAfter = parseRetryAfter(value, time.Now())
				break
			}
		}
	case errors.Is(err, io.ErrUnexpectedEOF) || errors.As(err, &malformed):
		f.Kind = InvalidResponse
	case errors.As(err, &network) || errors.Is(err, context.DeadlineExceeded):
		f.Kind = Network
	}
	if errors.Is(err, io.ErrUnexpectedEOF) {
		f.Message = "Model stream ended before a completion event"
	}
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
func parseRetryAfter(value string, now time.Time) *time.Duration {
	if seconds, err := strconv.ParseUint(strings.TrimSpace(value), 10, 32); err == nil {
		duration := time.Duration(seconds) * time.Second
		return &duration
	}
	if until, err := http.ParseTime(value); err == nil {
		duration := until.Sub(now)
		if duration < 0 {
			duration = 0
		}
		return &duration
	}
	return nil
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
	failure := Classify(err)
	if failure == nil {
		return nil
	}
	code := protocol.ErrorCodeUnavailable
	if failure.Kind == NotConfigured {
		code = protocol.ErrorCodeNotConfigured
	}
	reason := protocol.ModelFailureRequest
	switch failure.Kind {
	case Authentication:
		reason = protocol.ModelFailureUnauthorized
	case RateLimited, Quota:
		reason = protocol.ModelFailureRateLimited
	}
	return &protocol.Fault{Code: code, Message: failure.Message, Details: &protocol.FaultDetails{Kind: "model_request", Data: protocol.FaultDetailsModelRequestPayload{Reason: reason}}}
}
