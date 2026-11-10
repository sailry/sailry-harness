package plugins

import (
	"encoding/json"
	"net/url"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const MaxCards = 16
const MaxCardPropsBytes = 16 * 1024
const MaxCardSchemaBytes = 32 * 1024

func ToolCallIdentity(turn protocol.TurnID, reference protocol.PluginReference, tool, call string) protocol.RequestID {
	body, _ := json.Marshal([]any{turn, reference, tool, call})
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry plugin tool call v1", body)
	var identity protocol.RequestID
	copy(identity[:], digest[:16])
	identity[6] = identity[6]&0x0f | 0x80
	identity[8] = identity[8]&0x3f | 0x80
	return identity
}

func CardAccepts(declaration protocol.PluginCardDeclaration, raw json.RawMessage) bool {
	if !json.Valid(raw) {
		return false
	}
	value, err := decodeValue(raw)
	if err != nil {
		return false
	}
	object, ok := value.(map[string]any)
	if !ok || !publicCardProps(value) {
		return false
	}
	schemaValue, err := decodeValue(declaration.PropsSchema)
	if err != nil {
		return false
	}
	schema, ok := schemaValue.(map[string]any)
	if !ok {
		return false
	}
	properties, ok := schema["properties"].(map[string]any)
	if !ok {
		return false
	}
	if required, ok := schema["required"].([]any); ok {
		for _, entry := range required {
			name, ok := entry.(string)
			if _, exists := object[name]; !ok || !exists {
				return false
			}
		}
	}
	for name, item := range object {
		property, ok := properties[name].(map[string]any)
		if !ok {
			return false
		}
		switch property["type"] {
		case "string":
			text, ok := item.(string)
			maximum, bounded := cardUnsigned(property["maxLength"])
			if !ok || !bounded || uint64(utf8.RuneCountInString(text)) > maximum {
				return false
			}
			if minimum, bounded := cardUnsigned(property["minLength"]); bounded && uint64(utf8.RuneCountInString(text)) < minimum {
				return false
			}
		case "boolean":
			if _, ok := item.(bool); !ok {
				return false
			}
		case "integer":
			number, ok := cardSigned(item)
			if !ok {
				return false
			}
			if minimum, bounded := cardSigned(property["minimum"]); bounded && number < minimum {
				return false
			}
			if maximum, bounded := cardSigned(property["maximum"]); bounded && number > maximum {
				return false
			}
		default:
			return false
		}
		if raw, exists := property["enum"]; exists {
			choices, ok := raw.([]any)
			if !ok || !slices.ContainsFunc(choices, func(choice any) bool { return reflect.DeepEqual(choice, item) }) {
				return false
			}
		}
	}
	return true
}

func CardSummaryValid(summary protocol.PluginCardSummary) bool {
	if summary.Receipt.Block >= MaxCards || !cardIdentifier(summary.Key) ||
		summary.Package.Name == "" || len(summary.Package.Digest) != 64 || !summary.Fallback.Valid() || !json.Valid(summary.Props) {
		return false
	}
	for _, character := range []byte(summary.Package.Digest) {
		if !(character >= '0' && character <= '9' || character >= 'a' && character <= 'f' || character >= 'A' && character <= 'F') {
			return false
		}
	}
	value, err := decodeValue(summary.Props)
	return err == nil && publicCardProps(value)
}

func cardIdentifier(value string) bool {
	if value == "" || len(value) > 128 {
		return false
	}
	for _, character := range []byte(value) {
		if !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9' || character == '.' || character == '_' || character == '-') {
			return false
		}
	}
	return true
}

func publicCardName(value string) bool {
	return cardIdentifier(value) && !slices.Contains([]string{
		"token", "access_token", "refresh_token", "client_secret", "credential", "credentials",
		"password", "authorization", "authorization_url", "callback", "callback_url", "code_verifier",
		"pkce", "script", "html", "command",
	}, strings.ToLower(value))
}

func publicCardProps(value any) bool {
	object, ok := value.(map[string]any)
	if !ok || len(object) > 32 {
		return false
	}
	encoded, err := json.Marshal(value)
	if err != nil || len(encoded) > MaxCardPropsBytes {
		return false
	}
	for name, item := range object {
		if !publicCardName(name) {
			return false
		}
		switch item := item.(type) {
		case string:
			if len(item) > 4096 || cardAuthorizationURL(item) {
				return false
			}
		case json.Number:
			if _, signed := cardSigned(item); !signed {
				if _, unsigned := cardUnsigned(item); !unsigned {
					return false
				}
			}
		case nil, bool:
		default:
			return false
		}
	}
	return true
}

func cardSigned(value any) (int64, bool) {
	number, ok := value.(json.Number)
	if !ok {
		return 0, false
	}
	decoded, err := number.Int64()
	return decoded, err == nil
}

func cardUnsigned(value any) (uint64, bool) {
	number, ok := value.(json.Number)
	if !ok {
		return 0, false
	}
	decoded, err := strconv.ParseUint(string(number), 10, 64)
	return decoded, err == nil
}

func cardAuthorizationURL(value string) bool {
	target, err := url.Parse(value)
	if err != nil || target.Scheme == "" {
		return false
	}
	for name := range target.Query() {
		if slices.Contains([]string{"access_token", "refresh_token", "client_secret", "code_verifier", "code_challenge", "redirect_uri", "response_type"}, name) {
			return true
		}
	}
	return false
}
