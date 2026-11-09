package model

// Claude OAuth wire behavior derives from oh-my-pi c29f7c56 through Sailry's
// reviewed provider request adapter. MIT; see LICENSE.omp.

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"runtime"
	"sort"
	"strings"
	"unicode/utf16"

	"charm.land/fantasy"

	"github.com/cespare/xxhash/v2"
)

const claudeSeed uint64 = 0x4d659218e32a3268
const claudeVersion = "2.1.280"
const claudeIdentity = "You are Claude Code, Anthropic's official CLI for Claude."

type claudeTransport struct{ base http.RoundTripper }

func (transport claudeTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	if !strings.HasSuffix(request.URL.Path, "/v1/messages") {
		return transport.base.RoundTrip(request)
	}
	input, err := io.ReadAll(request.Body)
	if err != nil {
		return nil, err
	}
	request.Body.Close()
	var body map[string]any
	decoder := json.NewDecoder(bytes.NewReader(input))
	decoder.UseNumber()
	if err := decoder.Decode(&body); err != nil {
		return nil, err
	}
	var declarations struct {
		Tools []struct {
			InputSchema json.RawMessage `json:"input_schema"`
		} `json:"tools"`
		Messages []struct {
			Content []struct {
				Input json.RawMessage `json:"input"`
			} `json:"content"`
		} `json:"messages"`
	}
	if err := json.Unmarshal(input, &declarations); err != nil {
		return nil, err
	}
	if tools, ok := body["tools"].([]any); ok {
		for i := range tools {
			if i < len(declarations.Tools) {
				tools[i].(map[string]any)["input_schema"] = declarations.Tools[i].InputSchema
			}
		}
	}
	if tools, ok := body["tools"].([]any); ok {
		for _, value := range tools {
			tool := value.(map[string]any)
			if name, ok := tool["name"].(string); ok {
				tool["name"] = claudeToolName(name)
			}
		}
	}
	if messages, ok := body["messages"].([]any); ok {
		for i, value := range messages {
			message := value.(map[string]any)
			blocks, _ := message["content"].([]any)
			for j, value := range blocks {
				block := value.(map[string]any)
				if block["type"] == "tool_use" {
					if i < len(declarations.Messages) && j < len(declarations.Messages[i].Content) {
						block["input"] = declarations.Messages[i].Content[j].Input
					}
					if name, ok := block["name"].(string); ok {
						block["name"] = claudeToolName(name)
					}
				}
			}
		}
	}
	encoded, err := encodeClaude(body)
	if err != nil {
		return nil, err
	}
	request.Body = io.NopCloser(bytes.NewReader(encoded))
	request.ContentLength = int64(len(encoded))
	request.GetBody = func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(encoded)), nil }
	query := request.URL.Query()
	if !query.Has("beta") {
		query.Set("beta", "true")
		request.URL.RawQuery = query.Encode()
	}
	for key, value := range claudeHeaders() {
		request.Header.Set(key, value)
	}
	thinking, _ := body["thinking"].(map[string]any)
	active := thinking["type"] == "adaptive" || thinking["type"] == "enabled"
	tools, _ := body["tools"].([]any)
	agent := active || len(tools) > 0
	betas := []string{}
	if agent {
		betas = append(betas, "claude-code-20250219")
	}
	betas = append(betas, "oauth-2025-04-20", "interleaved-thinking-2025-05-14", "thinking-token-count-2026-05-13", "context-management-2025-06-27", "prompt-caching-scope-2026-01-05")
	if agent {
		betas = append(betas, "mid-conversation-system-2026-04-07")
		if active {
			betas = append(betas, "effort-2025-11-24")
		}
		betas = append(betas, "fallback-credit-2026-06-01")
	} else {
		betas = append(betas, "structured-outputs-2025-12-15")
	}
	request.Header.Set("anthropic-beta", strings.Join(betas, ","))
	return transport.base.RoundTrip(request)
}

func claudeToolName(name string) string {
	switch strings.ToLower(name) {
	case "web_search", "code_execution", "text_editor", "computer":
		return name
	default:
		return "_" + name
	}
}

func claudeHeaders() map[string]string {
	os := map[string]string{"darwin": "MacOS", "windows": "Windows", "linux": "Linux", "freebsd": "FreeBSD"}[runtime.GOOS]
	arch := map[string]string{"amd64": "x64", "arm64": "arm64", "386": "x86"}[runtime.GOARCH]
	if os == "" {
		os = runtime.GOOS
	}
	if arch == "" {
		arch = runtime.GOARCH
	}
	return map[string]string{
		"accept": "application/json", "content-type": "application/json", "anthropic-version": "2023-06-01",
		"anthropic-dangerous-direct-browser-access": "true", "x-app": "cli", "connection": "keep-alive", "accept-encoding": "gzip, deflate, br, zstd",
		"x-stainless-arch": arch, "x-stainless-lang": "js", "x-stainless-os": os, "x-stainless-package-version": "0.112.1",
		"x-stainless-retry-count": "0", "x-stainless-runtime": "node", "x-stainless-runtime-version": "v26.3.0", "x-stainless-timeout": "600",
		"user-agent": "claude-cli/2.1.280 (external, cli)",
	}
}

func claudeMetadata(installation, session, account string) map[string]any {
	var input string
	if account == "" {
		input = "omp-claude-device-id-v1:" + installation
	} else {
		input = "omp-claude-device-id-v2\x00" + installation + "\x00" + account
	}
	identity := struct {
		DeviceID    string `json:"device_id"`
		SessionID   string `json:"session_id"`
		AccountUUID string `json:"account_uuid,omitempty"`
	}{fmt.Sprintf("%x", sha256.Sum256([]byte(input))), session, account}
	return map[string]any{"user_id": string(claudeJSON(identity))}
}

func encodeClaude(body map[string]any) ([]byte, error) {
	first := ""
	messages, _ := body["messages"].([]any)
	for _, value := range messages {
		message, _ := value.(map[string]any)
		if message["role"] != "user" && message["role"] != "developer" {
			continue
		}
		if text, ok := message["content"].(string); ok && strings.TrimSpace(text) != "" {
			first = text
			break
		}
		if blocks, ok := message["content"].([]any); ok {
			image := false
			for _, value := range blocks {
				block, _ := value.(map[string]any)
				image = image || block["type"] == "image"
				if block["type"] == "text" {
					if text, ok := block["text"].(string); ok && strings.TrimSpace(text) != "" {
						first = text
						break
					}
				}
			}
			if first != "" || image {
				break
			}
		}
	}
	if captured, ok := body["sailry_claude_first_user"].(string); ok {
		first = captured
	}
	delete(body, "sailry_claude_first_user")
	version := claudeVersion
	if captured, ok := body["sailry_claude_version"].(string); ok {
		version = captured
	}
	delete(body, "sailry_claude_version")
	units := utf16.Encode([]rune(first))
	sampled := []uint16{'0', '0', '0'}
	for index, slot := range []int{4, 7, 20} {
		if slot < len(units) {
			sampled[index] = units[slot]
		}
	}
	fingerprint := fmt.Sprintf("%x", sha256.Sum256([]byte("59cf53e54c78"+string(utf16.Decode(sampled))+version)))
	system := []any{
		map[string]any{"type": "text", "text": "x-anthropic-billing-header: cc_version=" + version + "." + fingerprint[:3] + "; cc_entrypoint=cli; cch=00000;"},
		map[string]any{"type": "text", "text": claudeIdentity, "cache_control": map[string]any{"type": "ephemeral"}},
	}
	blocks, _ := body["system"].([]any)
	if text, ok := body["system"].(string); ok && text != "" {
		blocks = []any{map[string]any{"type": "text", "text": text}}
	}
	for _, value := range blocks {
		block, _ := value.(map[string]any)
		text, _ := block["text"].(string)
		if !strings.HasPrefix(text, "x-anthropic-billing-header:") && text != claudeIdentity {
			system = append(system, block)
		}
	}
	body["system"] = system
	if body["tools"] == nil {
		body["tools"] = []any{}
	}
	for _, value := range body["tools"].([]any) {
		tool := value.(map[string]any)
		if tool["type"] == nil || tool["type"] == "custom" {
			tool["eager_input_streaming"] = tool["name"] != "_yield"
		}
	}
	thinking, _ := body["thinking"].(map[string]any)
	if (thinking["type"] == "enabled" || thinking["type"] == "adaptive") && body["context_management"] == nil {
		body["context_management"] = map[string]any{"edits": []any{map[string]any{"type": "clear_thinking_20251015", "keep": "all"}}}
	}
	claudeCache(body)
	encoded := orderedClaude(body)
	marker := []byte(`"system":[{"type":"text","text":"x-anthropic-billing-header: cc_version=`)
	start := bytes.Index(encoded, marker)
	if start < 0 {
		return nil, fmt.Errorf("Claude billing block is missing")
	}
	offset := bytes.Index(encoded[start:], []byte("cch=00000"))
	if offset < 0 {
		return nil, fmt.Errorf("Claude billing checksum is missing")
	}
	digest := xxhash.NewWithSeed(claudeSeed)
	digest.Write(encoded)
	checksum := fmt.Sprintf("%05x", digest.Sum64()&0xfffff)
	copy(encoded[start+offset+4:], checksum)
	return encoded, nil
}

func claudeJSON(value any) []byte {
	var encoded bytes.Buffer
	encoder := json.NewEncoder(&encoded)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(value); err != nil {
		panic(err)
	}
	return bytes.TrimSuffix(encoded.Bytes(), []byte{'\n'})
}

func orderedClaude(body map[string]any) []byte {
	order := []string{"model", "messages", "system", "tools", "metadata", "max_tokens", "thinking", "context_management", "compaction", "output_config", "fallbacks", "anthropic_beta", "stream", "temperature", "top_p", "top_k", "stop_sequences", "speed", "tool_choice"}
	remaining := []string{}
	for key := range body {
		found := false
		for _, ordered := range order {
			found = found || ordered == key
		}
		if !found {
			remaining = append(remaining, key)
		}
	}
	sort.Strings(remaining)
	var encoded bytes.Buffer
	encoded.WriteByte('{')
	separator := ""
	for _, key := range append(order, remaining...) {
		value, exists := body[key]
		if !exists {
			continue
		}
		encoded.WriteString(separator)
		separator = ","
		encoded.Write(claudeJSON(key))
		encoded.WriteByte(':')
		prepared, err := sourceValue("anthropic", key, value, fantasy.Call{})
		if err != nil {
			panic(err)
		}
		encoded.Write(claudeJSON(prepared))
	}
	encoded.WriteByte('}')
	return encoded.Bytes()
}
