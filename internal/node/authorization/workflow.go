package authorization

import (
	"context"
	"encoding/json"
	"maps"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

const Deadline = 15 * time.Minute

type Input struct {
	Declaration  protocol.PluginModelAuthDeclaration
	Fields       map[string]string
	Redirect     string
	Callback     <-chan string
	Bundle       *plugins.ScriptBundle
	Previous     any
	HostID       string
	Registration *string
	Registered   func(context.Context, string) error
	Client       *http.Client
	Clock        clockwork.Clock
	Progress     func(protocol.ConversationLoginState) error
}

type Issued struct {
	AccountLabel *string           `json:"account_label,omitempty"`
	Access       string            `json:"access"`
	Refresh      *string           `json:"refresh"`
	ExpiresAtMs  *uint64           `json:"expires_at_ms"`
	Extra        map[string]string `json:"extra"`
}

type Result struct {
	Document any
	Issued   *Issued
	Grant    json.RawMessage
}

func Run(ctx context.Context, input Input) (Result, error) {
	if input.Clock == nil {
		input.Clock = clockwork.NewRealClock()
	}
	if input.Client == nil {
		input.Client = HTTPClient()
	}
	input.Fields = maps.Clone(input.Fields)
	if input.Fields == nil {
		input.Fields = map[string]string{}
	}
	if input.Progress == nil {
		input.Progress = func(protocol.ConversationLoginState) error { return invalid() }
	}
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	done := make(chan struct{})
	defer close(done)
	timer := input.Clock.NewTimer(Deadline)
	defer timer.Stop()
	go func() {
		select {
		case <-done:
		case <-operation.Done():
		case <-timer.Chan():
			cancel()
		}
	}()
	var document any
	var err error
	if input.Declaration.Flow.Kind == "script" {
		document, err = input.script(operation)
	} else {
		document, err = input.standard(operation)
	}
	if operation.Err() != nil {
		if ctx.Err() == nil {
			return Result{}, expired()
		}
		return Result{}, cancelled()
	}
	if err != nil {
		return Result{}, err
	}
	result := Result{Document: document}
	if input.Declaration.Flow.Kind == "script" && input.Declaration.Authentication != nil {
		body, err := json.Marshal(document)
		if err != nil {
			return Result{}, invalid()
		}
		if err := ValidateGrant(body, *input.Declaration.Authentication, input.Clock.Now()); err != nil {
			return Result{}, err
		}
		result.Grant = body
		return result, nil
	}
	if input.Declaration.Exchange != nil {
		if input.Bundle == nil {
			return Result{}, invalid()
		}
		value, err := plugins.Translate(operation, *input.Bundle, input.Declaration.Exchange.Handler, document, BodyLimit, time.Minute)
		if err != nil || json.Unmarshal(value, &document) != nil {
			return Result{}, invalid()
		}
	}
	if input.Declaration.Token == nil {
		return Result{}, invalid()
	}
	issued, err := ParseIssued(document, *input.Declaration.Token, uint64(input.Clock.Now().UnixMilli()), nil)
	result.Issued = &issued
	return result, err
}

func (input *Input) script(ctx context.Context) (any, error) {
	var flow protocol.PluginModelAuthFlowScriptPayload
	if decode(input.Declaration.Flow.Data, &flow) != nil || input.Bundle == nil {
		return nil, invalid()
	}
	verifier, proof, nonce := oauth2.GenerateVerifier(), oauth2.GenerateVerifier(), oauth2.GenerateVerifier()
	if input.Fields["user_agent"] == "" {
		input.Fields["user_agent"] = "Sailry/0.1"
	}
	initial := map[string]any{
		"previous": input.Previous, "host_id": input.HostID, "registration": input.Registration,
		"redirect": input.Redirect, "fields": input.Fields, "scopes": input.Declaration.Scopes,
		"proof": map[string]string{"verifier": verifier, "state": proof, "nonce": nonce, "challenge": oauth2.S256ChallengeFromVerifier(verifier)},
	}
	event := map[string]any{"kind": "start"}
	if input.Previous != nil {
		event["kind"] = "refresh"
	}
	var state any
	expires := input.Clock.Now().Add(Deadline)
	presented := false
	for {
		remaining := expires.Sub(input.Clock.Now())
		if remaining <= 0 {
			return nil, expired()
		}
		step := map[string]any{"context": initial, "now_ms": input.Clock.Now().UnixMilli(), "event": event, "state": state}
		body, err := plugins.Translate(ctx, *input.Bundle, flow.Handler, step, BodyLimit*4, min(time.Minute, remaining))
		if err != nil {
			return nil, err
		}
		var effect struct {
			Kind        string             `json:"kind"`
			Name        string             `json:"name"`
			Fields      map[string]string  `json:"fields"`
			State       any                `json:"state"`
			Optional    bool               `json:"optional"`
			URL         string             `json:"url"`
			UserCode    *string            `json:"user_code"`
			ExpiresAtMs uint64             `json:"expires_at_ms"`
			Proof       string             `json:"proof"`
			AllowCode   bool               `json:"allow_code"`
			DelayMs     uint64             `json:"delay_ms"`
			Token       string             `json:"token"`
			Audience    string             `json:"audience"`
			Nonce       *string            `json:"nonce"`
			ClientID    string             `json:"client_id"`
			Grant       any                `json:"grant"`
			Code        protocol.ErrorCode `json:"code"`
		}
		decoder := json.NewDecoder(strings.NewReader(string(body)))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&effect) != nil {
			return nil, invalid()
		}
		switch effect.Kind {
		case "request":
			endpoint, exists := flow.Requests[effect.Name]
			if !exists || effect.Optional && endpoint.Method != protocol.PluginModelAuthMethodGet {
				return nil, invalid()
			}
			fields := maps.Clone(input.Fields)
			for _, name := range []string{"client_id", "client_secret"} {
				configured, exists := input.Fields[name]
				requested, requestedExists := effect.Fields[name]
				if exists && requestedExists && configured != requested {
					return nil, invalid()
				}
			}
			maps.Copy(fields, effect.Fields)
			status, document, err := Response(ctx, input.Client, endpoint, fields, true)
			if err != nil {
				if !effect.Optional || ctx.Err() != nil {
					return nil, err
				}
				event = map[string]any{"kind": "failure", "code": protocol.ErrorCodeUnavailable}
			} else {
				event = map[string]any{"kind": "response", "status": status, "body": document}
			}
		case "prompt":
			target, err := url.Parse(effect.URL)
			if err != nil || target.Scheme != "https" || target.Host == "" || target.User != nil || target.Fragment != "" || presented {
				return nil, invalid()
			}
			expires = minTime(expires, time.UnixMilli(int64(effect.ExpiresAtMs)))
			if !expires.After(input.Clock.Now()) {
				return nil, expired()
			}
			var prompt protocol.ConversationLoginState
			if effect.UserCode != nil {
				if !bounded(*effect.UserCode, 256) {
					return nil, invalid()
				}
				prompt = protocol.ConversationLoginState{Kind: "pending", Data: protocol.ConversationLoginStatePendingPayload{
					VerificationUrl: effect.URL, UserCode: *effect.UserCode, ExpiresAtMs: uint64(expires.UnixMilli()),
				}}
			} else {
				prompt = protocol.ConversationLoginState{Kind: "browser", Data: protocol.ConversationLoginStateBrowserPayload{
					AuthorizationUrl: effect.URL, ExpiresAtMs: uint64(expires.UnixMilli()),
				}}
			}
			if err := input.Progress(prompt); err != nil {
				return nil, err
			}
			presented = true
			event = map[string]any{"kind": "presented"}
		case "callback":
			if flow.Callback == nil || input.Callback == nil || effect.AllowCode && !flow.Callback.Manual {
				return nil, invalid()
			}
			if err := ValidateRedirect(input.Redirect, *flow.Callback); err != nil {
				return nil, err
			}
			expected := proof
			if effect.Proof == "verifier" {
				expected = verifier
			} else if effect.Proof != "state" {
				return nil, invalid()
			}
			returned, err := input.callback(ctx, expires)
			if err != nil {
				return nil, err
			}
			fields, err := CallbackFields(returned, input.Redirect, expected, effect.AllowCode)
			if err != nil {
				return nil, err
			}
			event = map[string]any{"kind": "callback", "fields": fields}
		case "wait":
			if effect.DelayMs == 0 || effect.DelayMs > uint64(Deadline.Milliseconds()) {
				return nil, invalid()
			}
			if err := input.wait(ctx, min(time.Duration(effect.DelayMs)*time.Millisecond, remaining)); err != nil {
				return nil, err
			}
			event = map[string]any{"kind": "elapsed"}
		case "verify":
			if flow.Issuer == nil {
				return nil, invalid()
			}
			claims, err := VerifyIdentity(ctx, input.Client, *flow.Issuer, effect.Token, effect.Audience, effect.Nonce, input.Clock.Now)
			if err != nil {
				return nil, err
			}
			event = map[string]any{"kind": "identity", "claims": claims}
		case "registered":
			if !registration(effect.ClientID) || input.Registered == nil {
				return nil, invalid()
			}
			if err := input.Registered(ctx, effect.ClientID); err != nil {
				return nil, err
			}
			input.Registered = nil
			event = map[string]any{"kind": "registered"}
		case "finish":
			return effect.Grant, nil
		case "reject":
			switch effect.Code {
			case protocol.ErrorCodePermissionDenied:
				return nil, denied()
			case protocol.ErrorCodeExpired:
				return nil, expired()
			case protocol.ErrorCodeOutcomeUnknown:
				return nil, &protocol.Fault{Code: effect.Code, Message: "authorization refresh did not complete; sign in again"}
			default:
				return nil, invalid()
			}
		default:
			return nil, invalid()
		}
		state = effect.State
	}
}

func (input *Input) wait(ctx context.Context, duration time.Duration) error {
	timer := input.Clock.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return cancelled()
	case <-timer.Chan():
		return nil
	}
}

func (input *Input) callback(ctx context.Context, expires time.Time) (string, error) {
	timer := input.Clock.NewTimer(max(0, expires.Sub(input.Clock.Now())))
	defer timer.Stop()
	select {
	case value, open := <-input.Callback:
		if !open {
			return "", cancelled()
		}
		return value, nil
	case <-ctx.Done():
		return "", cancelled()
	case <-timer.Chan():
		return "", expired()
	}
}

func minTime(first, second time.Time) time.Time {
	if first.Before(second) {
		return first
	}
	return second
}

func registration(value string) bool {
	return value != "" && len(value) <= 256 && value != "dynamic_agent_client" && !strings.ContainsFunc(value, func(character rune) bool {
		return !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '-' || character == '_')
	})
}

func ValidateRedirect(value string, declaration protocol.PluginModelAuthCallback) error {
	if declaration.Port == nil && declaration.Manual && declaration.RedirectUri != nil && value == *declaration.RedirectUri {
		return nil
	}
	target, err := url.Parse(value)
	if err != nil || target.Scheme != "http" || target.Hostname() != "127.0.0.1" && !(declaration.AllowLocalhost && target.Hostname() == "localhost") || target.Port() == "" || target.Port() == "0" || target.Path != declaration.Path || target.User != nil || target.RawQuery != "" || target.Fragment != "" || target.String() != value {
		return invalid()
	}
	if declaration.Port != nil && (target.Port() != strconv.FormatUint(uint64(*declaration.Port), 10) || declaration.RedirectUri != nil && *declaration.RedirectUri != value) {
		return invalid()
	}
	return nil
}

func CallbackFields(input, redirect, expected string, allowCode bool) (map[string]string, error) {
	value := strings.TrimSpace(input)
	fields := map[string]string{}
	decodeQuery := func(query string) error {
		values, err := url.ParseQuery(query)
		if err != nil {
			return invalid()
		}
		for name, values := range values {
			if len(values) != 1 {
				return invalid()
			}
			fields[name] = values[0]
		}
		return nil
	}
	if strings.HasPrefix(value, "http://") || strings.HasPrefix(value, "https://") {
		target, err := url.Parse(value)
		if err != nil || target.Fragment != "" || decodeQuery(target.RawQuery) != nil {
			return nil, invalid()
		}
		target.RawQuery, target.ForceQuery = "", false
		if target.String() != redirect {
			return nil, invalid()
		}
	} else if allowCode {
		if strings.Contains(value, "code=") {
			if err := decodeQuery(value); err != nil {
				return nil, err
			}
		} else {
			code, proof, exists := strings.Cut(value, "#")
			fields["code"] = code
			if exists {
				fields["state"] = proof
			}
		}
	} else {
		return nil, invalid()
	}
	proof, exists := fields["state"]
	if proof != expected && (!allowCode || exists) {
		return nil, invalid()
	}
	if _, exists := fields["error"]; exists {
		return nil, denied()
	}
	code := fields["code"]
	if !bounded(code, 16384) || strings.ContainsFunc(code, unicode.IsSpace) {
		return nil, invalid()
	}
	return fields, nil
}
