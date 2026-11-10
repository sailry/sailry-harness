package authorization

import (
	"encoding/json"
	"math"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
)

func ParseIssued(document any, mapping protocol.PluginModelAuthMapping, now uint64, previous *Issued) (Issued, error) {
	result := Issued{Extra: map[string]string{}}
	var err error
	result.Access, err = text(document, mapping.Access)
	if err != nil || !validToken(result.Access) {
		return result, invalid()
	}
	if previous != nil {
		result.Refresh = previous.Refresh
		result.AccountLabel = previous.AccountLabel
		for name, value := range previous.Extra {
			result.Extra[name] = value
		}
	}
	if mapping.Refresh != nil {
		value, err := text(document, *mapping.Refresh)
		if err != nil {
			return result, err
		}
		if value != "" {
			result.Refresh = &value
		}
	}
	if mapping.ExpiresIn != nil {
		duration, ok := secondsValue(pointer(document, *mapping.ExpiresIn))
		if !ok || uint64(duration.Milliseconds()) > math.MaxInt64-now {
			return result, invalid()
		}
		expiry := now + uint64(duration.Milliseconds())
		result.ExpiresAtMs = &expiry
	}
	for name, path := range mapping.Extra {
		value, err := text(document, path)
		if err != nil {
			return result, err
		}
		if value != "" {
			result.Extra[name] = value
		}
	}
	if mapping.AccountLabel != nil {
		if value, ok := pointer(document, *mapping.AccountLabel).(string); ok && bounded(strings.TrimSpace(value), 1024) {
			label := strings.TrimSpace(value)
			result.AccountLabel = &label
		}
	}
	return result, nil
}

func (issued Issued) Fields() map[string]string {
	values := map[string]string{"access": issued.Access}
	if issued.Refresh != nil {
		values["refresh"] = *issued.Refresh
	}
	for name, value := range issued.Extra {
		values["extra."+name] = value
	}
	return values
}

func (issued Issued) Headers(injections []protocol.PluginModelAuthInjection) (http.Header, error) {
	fields := issued.Fields()
	headers := http.Header{}
	for _, injection := range injections {
		value, err := Expand(injection.Template, fields)
		if err != nil || !httpguts.ValidHeaderFieldName(injection.Header) || !httpguts.ValidHeaderFieldValue(value) || headers.Get(injection.Header) != "" {
			return nil, invalid()
		}
		switch strings.ToLower(injection.Header) {
		case "host", "connection", "content-length", "transfer-encoding", "proxy-authorization", "upgrade":
			return nil, invalid()
		}
		headers.Set(injection.Header, value)
	}
	return headers, nil
}

func validToken(value string) bool {
	return value != "" && len(value) <= 16384 && httpguts.ValidHeaderFieldValue(value)
}

func ValidateGrant(body []byte, authentication protocol.Authentication, now time.Time) error {
	var value struct {
		Kind      protocol.Authentication `json:"kind"`
		Access    string                  `json:"access"`
		Refresh   string                  `json:"refresh"`
		Expiry    uint64                  `json:"expires_at_ms"`
		Subject   string                  `json:"subject"`
		AccountID *string                 `json:"account_id"`
		Email     *string                 `json:"email"`
		ClientID  string                  `json:"client_id"`
		Identity  string                  `json:"id_token"`
		Scopes    []string                `json:"scopes"`
		Github    string                  `json:"github"`
		Endpoint  string                  `json:"endpoint"`
	}
	if json.Unmarshal(body, &value) != nil || value.Kind != authentication || !validToken(value.Access) || value.Expiry == 0 || value.Expiry > math.MaxInt64 {
		return invalid()
	}
	renewal := value.Refresh
	if authentication == protocol.AuthenticationCopilot {
		renewal = value.Github
		target, err := url.Parse(value.Endpoint)
		if err != nil || target.Scheme != "https" || target.User != nil || target.RawQuery != "" || target.Fragment != "" ||
			target.Hostname() != "api.githubcopilot.com" && !strings.HasSuffix(target.Hostname(), ".githubcopilot.com") {
			return invalid()
		}
	}
	if !validToken(renewal) {
		return invalid()
	}
	if value.Expiry <= uint64(now.UnixMilli()) {
		return expired()
	}
	if authentication == protocol.AuthenticationChatGpt || authentication == protocol.AuthenticationChatGptOAuth {
		if !bounded(value.Subject, 2048) || !validToken(value.Identity) {
			return invalid()
		}
		if authentication == protocol.AuthenticationChatGpt && !registration(value.ClientID) {
			return invalid()
		}
		if authentication == protocol.AuthenticationChatGptOAuth && value.AccountID == nil {
			return invalid()
		}
		if value.AccountID != nil && (!bounded(*value.AccountID, 2048) || !httpguts.ValidHeaderFieldValue(*value.AccountID)) {
			return invalid()
		}
	}
	if value.Email != nil && !bounded(*value.Email, 1024) || len(value.Scopes) > 128 {
		return invalid()
	}
	for _, scope := range value.Scopes {
		if scope == "" || len(scope) > 256 || strings.ContainsFunc(scope, func(character rune) bool {
			return !(character == 0x21 || character >= 0x23 && character <= 0x5b || character >= 0x5d && character <= 0x7e)
		}) {
			return invalid()
		}
	}
	return nil
}
