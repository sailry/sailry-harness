package externalbrowser

import (
	"bytes"
	"encoding/json"
	"math"
	"net/url"
	"sort"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	argumentBytes = 64 * 1024
	resultBytes   = 48 * 1024
	resultEntries = 128
)

// Validate checks the arguments every action shares and returns them decoded.
func Validate(action protocol.ExternalBrowserAction, arguments json.RawMessage) (map[string]any, error) {
	invalid := func(message string) (map[string]any, error) {
		return nil, fault(protocol.ErrorCodeInvalidRequest, message)
	}
	if _, err := action.MarshalJSON(); err != nil {
		return invalid("unknown browser action")
	}
	var values map[string]any
	var compact bytes.Buffer
	if json.Compact(&compact, arguments) != nil || compact.Len() > argumentBytes || json.Unmarshal(arguments, &values) != nil || values == nil {
		return invalid("browser arguments must be an object of at most 64 KiB")
	}
	if action == protocol.ExternalBrowserActionWait {
		seconds, ok := values["seconds"].(float64)
		if !ok {
			return invalid("browser wait seconds must be a non-negative representable duration")
		}
		if _, err := waitDuration(seconds); err != nil {
			return nil, err
		}
	}
	if text, ok := values["url"].(string); ok {
		parsed, err := url.Parse(text)
		if err != nil || parsed.Scheme != "http" && parsed.Scheme != "https" || parsed.Host == "" || parsed.User != nil {
			return invalid("external browser URLs must use HTTP or HTTPS without credentials")
		}
	}
	return values, nil
}

func waitDuration(seconds float64) (time.Duration, error) {
	if math.IsNaN(seconds) || seconds < 0 || seconds > float64(math.MaxInt64/int64(time.Second)) {
		return 0, fault(protocol.ErrorCodeInvalidRequest, "browser wait seconds must be a non-negative representable duration")
	}
	return time.Duration(seconds * float64(time.Second)), nil
}

// bound limits a result to the remaining byte budget and to 128 entries per
// collection. It reports whether anything was dropped.
func bound(value any, remaining *int) (any, bool) {
	switch current := value.(type) {
	case string:
		end := min(len(current), *remaining)
		for end > 0 && end < len(current) && !utf8.RuneStart(current[end]) {
			end--
		}
		*remaining -= end
		return current[:end], end < len(current)
	case []any:
		truncated := len(current) > resultEntries
		current = current[:min(len(current), resultEntries)]
		bounded := make([]any, len(current))
		for index, entry := range current {
			var cut bool
			bounded[index], cut = bound(entry, remaining)
			truncated = truncated || cut
		}
		return bounded, truncated
	case map[string]any:
		keys := make([]string, 0, len(current))
		for key := range current {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		bounded := make(map[string]any, len(keys))
		truncated := false
		for index, key := range keys {
			if index >= resultEntries || len(key) > *remaining {
				truncated = true
				continue
			}
			*remaining -= len(key)
			var cut bool
			bounded[key], cut = bound(current[key], remaining)
			truncated = truncated || cut
		}
		return bounded, truncated
	}
	return value, false
}

// normalize converts typed protocol values into the plain JSON shapes that
// bound understands.
func normalize(value any) any {
	encoded, err := json.Marshal(value)
	if err != nil {
		return nil
	}
	var plain any
	if json.Unmarshal(encoded, &plain) != nil {
		return nil
	}
	return plain
}

func argument[T any](arguments map[string]any, name string) (T, bool) {
	value, ok := arguments[name].(T)
	return value, ok
}

func required(arguments map[string]any, name string) (string, error) {
	value, ok := arguments[name].(string)
	if !ok {
		return "", unavailable("Missing '" + name + "' parameter")
	}
	return value, nil
}

func integer(arguments map[string]any, name string, fallback int64) int64 {
	if value, ok := arguments[name].(float64); ok && value == math.Trunc(value) && math.Abs(value) < 1<<53 {
		return int64(value)
	}
	return fallback
}

func boolean(arguments map[string]any, name string, fallback bool) bool {
	if value, ok := arguments[name].(bool); ok {
		return value
	}
	return fallback
}

func textList(arguments map[string]any, name string) []string {
	values, _ := arguments[name].([]any)
	texts := []string{}
	for _, value := range values {
		if text, ok := value.(string); ok {
			texts = append(texts, text)
		}
	}
	return texts
}
