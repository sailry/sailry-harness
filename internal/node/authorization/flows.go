package authorization

import (
	"context"
	"encoding/json"
	"math"
	"net/url"
	"reflect"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

func (input *Input) standard(ctx context.Context) (any, error) {
	declaration := input.Declaration
	fields := input.Fields
	request := func(endpoint protocol.PluginModelAuthEndpoint) (any, error) {
		status, document, err := Response(ctx, input.Client, endpoint, fields, false)
		if err != nil {
			return nil, err
		}
		switch classify(status, document) {
		case "success":
			return document, nil
		case "rejected":
			return nil, denied()
		default:
			return nil, unavailable()
		}
	}
	switch declaration.Flow.Kind {
	case "token_exchange":
		var flow protocol.PluginModelAuthFlowTokenExchangePayload
		if decode(declaration.Flow.Data, &flow) != nil || input.Callback != nil || input.Redirect != "" {
			return nil, invalid()
		}
		return request(flow.Token)
	case "pkce":
		var flow protocol.PluginModelAuthFlowPkcePayload
		if decode(declaration.Flow.Data, &flow) != nil || input.Callback == nil || fields["client_id"] == "" {
			return nil, invalid()
		}
		if flow.Redirect == protocol.PluginModelAuthRedirectLoopback {
			if ValidateRedirect(input.Redirect, protocol.PluginModelAuthCallback{Path: "/auth/callback"}) != nil {
				return nil, invalid()
			}
		} else {
			target, err := url.Parse(input.Redirect)
			if err != nil || target.Scheme != "https" || target.Host == "" || target.User != nil || target.RawQuery != "" || target.Fragment != "" {
				return nil, invalid()
			}
		}
		authorize, err := url.Parse(flow.Authorize)
		if err != nil || authorize.Scheme != "https" || authorize.Host == "" || authorize.User != nil {
			return nil, invalid()
		}
		for _, name := range []string{"client_id", "redirect_uri", "response_type", "state", "code_challenge", "code_challenge_method", "scope"} {
			if authorize.Query().Has(name) {
				return nil, invalid()
			}
		}
		verifier, proof := oauth2.GenerateVerifier(), oauth2.GenerateVerifier()
		config := oauth2.Config{ClientID: fields["client_id"], RedirectURL: input.Redirect, Scopes: declaration.Scopes, Endpoint: oauth2.Endpoint{AuthURL: flow.Authorize}}
		expires := input.Clock.Now().Add(Deadline)
		if err := input.Progress(protocol.ConversationLoginState{Kind: "browser", Data: protocol.ConversationLoginStateBrowserPayload{
			AuthorizationUrl: config.AuthCodeURL(proof, oauth2.S256ChallengeOption(verifier)), ExpiresAtMs: uint64(expires.UnixMilli()),
		}}); err != nil {
			return nil, err
		}
		returned, err := input.callback(ctx, expires)
		if err != nil {
			return nil, err
		}
		callback, err := CallbackFields(returned, input.Redirect, proof, false)
		if err != nil {
			return nil, err
		}
		fields["code"], fields["redirect_uri"], fields["verifier"] = callback["code"], input.Redirect, verifier
		return request(form(flow.Token, map[string]string{
			"grant_type": "authorization_code", "code": "{code}", "redirect_uri": "{redirect_uri}", "code_verifier": "{verifier}",
		}, fields))
	case "device_code":
		var flow protocol.PluginModelAuthFlowDeviceCodePayload
		if decode(declaration.Flow.Data, &flow) != nil || input.Callback != nil || input.Redirect != "" {
			return nil, invalid()
		}
		if len(declaration.Scopes) > 0 {
			fields["scope"] = strings.Join(declaration.Scopes, " ")
		}
		document, err := request(form(flow.Device, nil, fields))
		if err != nil {
			return nil, err
		}
		verification, err := text(document, "/verification_uri")
		if err != nil {
			return nil, err
		}
		code, err := text(document, "/device_code")
		if err != nil || code == "" {
			return nil, invalid()
		}
		userCode, err := text(document, "/user_code")
		if err != nil || !bounded(userCode, 256) {
			return nil, invalid()
		}
		seconds, ok := secondsValue(pointer(document, "/expires_in"))
		if !ok {
			return nil, invalid()
		}
		expires := input.Clock.Now().Add(seconds)
		target, err := url.Parse(verification)
		if err != nil || target.Scheme != "https" || target.Host == "" || target.User != nil {
			return nil, invalid()
		}
		interval := 5 * time.Second
		if value := pointer(document, "/interval"); value != nil {
			interval, ok = secondsValue(value)
			if !ok {
				return nil, invalid()
			}
		}
		if err := input.Progress(protocol.ConversationLoginState{Kind: "pending", Data: protocol.ConversationLoginStatePendingPayload{
			VerificationUrl: verification, UserCode: userCode, ExpiresAtMs: uint64(expires.UnixMilli()),
		}}); err != nil {
			return nil, err
		}
		fields["device_code"] = code
		endpoint := form(flow.Token, map[string]string{"grant_type": "urn:ietf:params:oauth:grant-type:device_code", "device_code": "{device_code}"}, fields)
		for input.Clock.Now().Before(expires) {
			if err := input.wait(ctx, interval); err != nil {
				return nil, err
			}
			if !input.Clock.Now().Before(expires) {
				return nil, expired()
			}
			status, document, err := Response(ctx, input.Client, endpoint, fields, false)
			if err != nil {
				if ctx.Err() != nil {
					return nil, err
				}
				continue
			}
			switch classify(status, document) {
			case "pending", "temporary":
				continue
			case "slow_down":
				interval += 5 * time.Second
			case "success":
				return document, nil
			case "rejected":
				return nil, denied()
			default:
				return nil, invalid()
			}
		}
		return nil, expired()
	case "polling":
		var flow protocol.PluginModelAuthFlowPollingPayload
		if decode(declaration.Flow.Data, &flow) != nil || input.Callback != nil || input.Redirect != "" {
			return nil, invalid()
		}
		document, err := request(flow.Start)
		if err != nil {
			return nil, err
		}
		state, err := text(document, flow.State)
		if err != nil || state == "" {
			return nil, invalid()
		}
		link, err := text(document, flow.VerificationUrl)
		target, parseErr := url.Parse(link)
		if err != nil || parseErr != nil || target.Scheme != "https" || target.Host == "" || target.User != nil {
			return nil, invalid()
		}
		expires := input.Clock.Now().Add(Deadline)
		if err := input.Progress(protocol.ConversationLoginState{Kind: "browser", Data: protocol.ConversationLoginStateBrowserPayload{
			AuthorizationUrl: link, ExpiresAtMs: uint64(expires.UnixMilli()),
		}}); err != nil {
			return nil, err
		}
		fields["state"] = state
		interval := 5 * time.Second
		if flow.IntervalMs != nil {
			if *flow.IntervalMs == 0 || *flow.IntervalMs > uint64(Deadline.Milliseconds()) {
				return nil, invalid()
			}
			interval = time.Duration(*flow.IntervalMs) * time.Millisecond
		}
		for {
			if err := input.wait(ctx, interval); err != nil {
				return nil, err
			}
			status, document, err := Response(ctx, input.Client, flow.Token, fields, false)
			if err != nil {
				return nil, err
			}
			switch classify(status, document) {
			case "rejected":
				return nil, denied()
			case "success":
				for _, expected := range flow.Success.Values {
					var value any
					if json.Unmarshal(expected, &value) == nil && reflect.DeepEqual(value, pointer(document, flow.Success.Pointer)) {
						return document, nil
					}
				}
			case "invalid":
				return nil, invalid()
			}
		}
	default:
		return nil, invalid()
	}
}

func form(endpoint protocol.PluginModelAuthEndpoint, defaults, fields map[string]string) protocol.PluginModelAuthEndpoint {
	if endpoint.Body != nil {
		return endpoint
	}
	values := map[string]string{}
	for name, value := range defaults {
		values[name] = value
	}
	for _, name := range []string{"client_id", "client_secret", "scope"} {
		if _, exists := fields[name]; exists {
			values[name] = "{" + name + "}"
		}
	}
	endpoint.Body = &protocol.PluginModelAuthBody{Kind: "form", Data: protocol.PluginModelAuthBodyFormPayload{Fields: values}}
	return endpoint
}

func classify(status int, document any) string {
	if status == 401 {
		return "rejected"
	}
	if status >= 500 {
		return "temporary"
	}
	switch pointer(document, "/error") {
	case "invalid_grant", "bad_refresh_token", "invalid_client", "unauthorized_client", "access_denied", "expired_token":
		return "rejected"
	case "authorization_pending":
		return "pending"
	case "slow_down":
		return "slow_down"
	case "temporarily_unavailable", "server_error":
		return "temporary"
	case nil:
		if status >= 200 && status < 300 {
			return "success"
		}
		return "temporary"
	default:
		return "invalid"
	}
}

func secondsValue(input any) (time.Duration, bool) {
	value, ok := input.(float64)
	if !ok || value <= 0 || math.Trunc(value) != value || value > float64(math.MaxInt64/int64(time.Second)) {
		return 0, false
	}
	return time.Duration(value) * time.Second, true
}
