package web

import (
	"bufio"
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"slices"
	"strings"
	"time"

	"github.com/openai/openai-go/v3/packages/ssestream"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func nativeRequestFault(code protocol.ErrorCode, reason protocol.ModelFailure, message string, temporary bool) *searchFailure {
	return &searchFailure{fault: &protocol.Fault{Code: code, Message: message, Details: &protocol.FaultDetails{Kind: "model_request", Data: protocol.FaultDetailsModelRequestPayload{Reason: reason}}}, temporary: temporary}
}
func nativeInvalid(message string) *searchFailure {
	return nativeRequestFault(protocol.ErrorCodeInvalidRequest, protocol.ModelFailureInvalidResponse, message, false)
}
func nativeUnsupported() *searchFailure {
	return &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "model service does not support native web search", Details: &protocol.FaultDetails{Kind: "search_unsupported"}}}
}
func nativeInterrupted() *searchFailure {
	return nativeRequestFault(protocol.ErrorCodeUnavailable, protocol.ModelFailureInvalidResponse, "model search response was interrupted", true)
}
func nativeLimited(retry time.Time) *searchFailure {
	failure := nativeRequestFault(protocol.ErrorCodeUnavailable, protocol.ModelFailureRateLimited, "model search rate limit reached", true)
	failure.retryAt = &retry
	return failure
}

func nativeRead(response *http.Response, api protocol.ConversationModelApi, now time.Time) (map[string]any, error) {
	if response.ContentLength > searchBodyLimit {
		return nil, nativeInvalid("model search response is too large")
	}
	success := response.StatusCode >= 200 && response.StatusCode < 300
	contentType := response.Header.Get("Content-Type")
	streaming := strings.EqualFold(strings.TrimSpace(strings.Split(contentType, ";")[0]), "text/event-stream")
	if success && api == protocol.ConversationModelApiResponses {
		reader := bufio.NewReader(response.Body)
		if contentType == "" {
			// Preserve the prefix for the SDK decoder while identifying an untyped
			// event stream. Whitespace counts toward the same total body bound.
			prefix := make([]byte, 0, 32)
			for len(prefix) <= searchBodyLimit {
				value, err := reader.ReadByte()
				if err != nil {
					if err != io.EOF {
						return nil, nativeInterrupted()
					}
					break
				}
				prefix = append(prefix, value)
				if !bytes.ContainsRune([]byte(" \t\r\n"), rune(value)) {
					streaming = value == 'd' || value == 'e' || value == ':'
					break
				}
			}
			if len(prefix) > searchBodyLimit {
				return nil, nativeInvalid("model search response is too large")
			}
			response.Body = &nativeReadCloser{Reader: io.MultiReader(bytes.NewReader(prefix), reader), Closer: response.Body}
		} else {
			response.Body = &nativeReadCloser{Reader: reader, Closer: response.Body}
		}
		if streaming {
			return nativeEvents(response, now)
		}
	}
	encoded, err := io.ReadAll(io.LimitReader(response.Body, searchBodyLimit+1))
	if err != nil {
		return nil, nativeInterrupted()
	}
	if len(encoded) > searchBodyLimit {
		return nil, nativeInvalid("model search response is too large")
	}
	value, decodeErr := nativeDecode(encoded)
	if !success {
		return nil, nativeRejected(response.StatusCode, value, retryDeadline(response.Header.Get("Retry-After"), now))
	}
	if decodeErr != nil {
		return nil, nativeInvalid("model search service returned invalid JSON")
	}
	return value, nil
}

type nativeReadCloser struct {
	io.Reader
	io.Closer
}

// nativeStreamBody bounds total bytes consumed by the official SSE decoder,
// including comments and unknown events. A single large event cannot bypass it.
type nativeStreamBody struct {
	io.ReadCloser
	remaining int64
}

func (body *nativeStreamBody) Read(target []byte) (int, error) {
	if int64(len(target)) > body.remaining+1 {
		target = target[:body.remaining+1]
	}
	count, err := body.ReadCloser.Read(target)
	body.remaining -= int64(count)
	if body.remaining < 0 {
		return 0, io.ErrUnexpectedEOF
	}
	return count, err
}

func nativeEvents(response *http.Response, now time.Time) (map[string]any, error) {
	stream := *response
	stream.Header = response.Header.Clone()
	stream.Header.Set("Content-Type", "text/event-stream")
	stream.Body = &nativeStreamBody{ReadCloser: response.Body, remaining: searchBodyLimit}
	decoder := ssestream.NewDecoder(&stream)
	defer decoder.Close()
	output := map[uint64]map[string]any{}
	for decoder.Next() {
		event := decoder.Event()
		if string(event.Data) == "[DONE]" {
			break
		}
		value, err := nativeDecode(event.Data)
		if err != nil {
			return nil, nativeInvalid("model search stream contains invalid JSON")
		}
		kind, present := value["type"].(string)
		if !present {
			kind = event.Type
		}
		switch kind {
		case "response.output_item.done":
			index, ok := jsonUint(value["output_index"])
			if !ok {
				return nil, nativeInvalid("model search output index is missing")
			}
			item, ok := value["item"].(map[string]any)
			if !ok {
				return nil, nativeInvalid("model search output item is missing")
			}
			output[index] = item
		case "response.completed":
			completed, ok := value["response"].(map[string]any)
			if !ok {
				return nil, nativeInvalid("model search stream has no completed response")
			}
			items, array := completed["output"].([]any)
			if len(output) > 0 && (completed["output"] == nil || array && len(items) == 0) {
				indexes := make([]uint64, 0, len(output))
				for index := range output {
					indexes = append(indexes, index)
				}
				slices.Sort(indexes)
				items = make([]any, 0, len(indexes))
				for _, index := range indexes {
					items = append(items, output[index])
				}
				completed["output"] = items
			}
			return completed, nil
		case "response.failed":
			return nil, nativeUpstream(jsonAt(value, "response", "error"), now)
		case "error":
			upstream := any(value)
			if nested, ok := value["error"]; ok {
				upstream = nested
			}
			return nil, nativeUpstream(upstream, now)
		case "response.incomplete":
			return nil, nativeInvalid("model search response did not complete")
		}
	}
	return nil, nativeInterrupted()
}

func nativeDecode(encoded []byte) (map[string]any, error) {
	decoder := json.NewDecoder(bytes.NewReader(encoded))
	decoder.UseNumber()
	var value map[string]any
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return nil, io.ErrUnexpectedEOF
	}
	return value, nil
}

func nativeRejected(status int, value map[string]any, retry time.Time) *searchFailure {
	if status == http.StatusTooManyRequests {
		return nativeLimited(retry)
	}
	upstream := any(value)
	if nested, present := value["error"]; present {
		upstream = nested
	}
	if slices.Contains([]int{400, 403, 404, 422, 501}, status) && nativeUnsupportedError(upstream) {
		return nativeUnsupported()
	}
	if status == http.StatusUnauthorized || status == http.StatusForbidden {
		return nativeRequestFault(protocol.ErrorCodePermissionDenied, protocol.ModelFailureUnauthorized, "model search authentication failed", false)
	}
	if status >= 500 && status < 600 {
		return nativeRequestFault(protocol.ErrorCodeUnavailable, protocol.ModelFailureRequest, "model search service is unavailable", true)
	}
	return nativeRequestFault(protocol.ErrorCodeInvalidRequest, protocol.ModelFailureRequest, "model search service rejected the request", false)
}

func nativeErrorCode(value any, keys ...string) string {
	object, _ := value.(map[string]any)
	for _, key := range keys {
		if code, present := object[key]; present {
			return jsonString(code)
		}
	}
	return ""
}
func nativeUpstream(value any, now time.Time) *searchFailure {
	if nativeUnsupportedError(value) {
		return nativeUnsupported()
	}
	switch nativeErrorCode(value, "error_code", "code", "type", "status") {
	case "too_many_requests", "rate_limit_error", "rate_limit_exceeded", "RESOURCE_EXHAUSTED":
		return nativeLimited(now.Add(time.Minute))
	case "authentication_error", "invalid_api_key", "UNAUTHENTICATED":
		return nativeRequestFault(protocol.ErrorCodePermissionDenied, protocol.ModelFailureUnauthorized, "model search authentication failed", false)
	case "unavailable", "api_error", "server_error", "overloaded_error", "UNAVAILABLE":
		return nativeRequestFault(protocol.ErrorCodeUnavailable, protocol.ModelFailureRequest, "model search service is unavailable", true)
	}
	return nativeRequestFault(protocol.ErrorCodeInvalidRequest, protocol.ModelFailureRequest, "native web search failed", false)
}
func nativeUnsupportedError(value any) bool {
	code := nativeErrorCode(value, "code", "type", "status")
	if slices.Contains([]string{"unsupported_tool", "tool_not_supported", "web_search_not_supported"}, code) {
		return true
	}
	parameter := strings.ToLower(jsonString(jsonAt(value, "param")))
	if code == "unsupported_parameter" && parameter == "tools" {
		return true
	}
	message := strings.ToLower(jsonString(jsonAt(value, "message")))
	message = strings.Map(func(character rune) rune {
		switch character {
		case '\'', '"', '`', ':':
			return ' '
		}
		return character
	}, message)
	message = strings.Join(strings.Fields(message), " ")
	for _, name := range []string{"web_search", "web_search_20250305", "web search", "google_search", "googlesearch", "google search", "search grounding", "grounding with google search"} {
		for _, suffix := range []string{"", " tool"} {
			for _, predicate := range []string{"not supported", "not available", "not enabled", "not allowed"} {
				if strings.Contains(message, name+suffix+" "+predicate) || strings.Contains(message, name+suffix+" is "+predicate) {
					return true
				}
			}
		}
		for _, predicate := range []string{"does not support", "unsupported tool", "unknown tool", "unknown name", "unsupported value"} {
			if strings.Contains(message, predicate+" "+name) {
				return true
			}
		}
	}
	return false
}
