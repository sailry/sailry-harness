package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const cacheToolsOutputLimit int64 = 64

type CacheToolsSample struct {
	Group         string  `json:"group"`
	Phase         string  `json:"phase"`
	Index         int     `json:"index"`
	Input         *uint64 `json:"input"`
	Cached        *uint64 `json:"cached"`
	Fresh         *uint64 `json:"fresh"`
	ResponseModel string  `json:"response_model"`
}

type cacheToolsReceiptKey struct{}

func captureCacheToolsResponseModel(ctx context.Context, response map[string]any) {
	if receipt, ok := ctx.Value(cacheToolsReceiptKey{}).(*string); ok {
		if name := wireString(response["model"]); name != "" {
			*receipt = name
		}
	}
}

func RunCacheToolsAcceptance(ctx context.Context, binding Binding) ([]CacheToolsSample, error) {
	type probe struct {
		model  fantasy.LanguageModel
		call   fantasy.Call
		sample CacheToolsSample
	}
	var probes [3][2]probe
	limit := cacheToolsOutputLimit
	if listed := OutputLimit(binding, false); listed > 0 {
		limit = min(limit, listed)
	}
	namespace, reference := protocol.NewSessionID().String(), cacheToolsReference()
	for groupIndex, group := range []string{"without_tools", "with_tools"} {
		binding.Session = protocol.NewSessionID()
		config, err := transportBinding(binding)
		if err != nil || config.Kind != "responses" {
			return nil, errors.New("Could not prepare the Responses transport")
		}
		languageModel, err := newSDK(ctx, config)
		if err != nil {
			return nil, errors.New("Could not prepare the Responses SDK")
		}
		var request Request
		request.System = "This is a synthetic cache benchmark. Do not call tools. Reply with OK only."
		request.MaxOutputTokens = &limit
		if groupIndex == 1 {
			request.Tools, request.ToolSchemas = cacheToolsDeclarations()
		}
		prefix := fmt.Sprintf("Synthetic group %c for run %s\n", 'A'+groupIndex, namespace) + reference
		prefixRecord := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, prefix)), time.Now())
		for index, phase := range []string{"cold", "warm", "warm"} {
			prompt := fmt.Sprintf("Sample marker %d. Reply with OK only.", index+1)
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, prompt)), time.Now())
			request.Records = []canonical.Record{prefixRecord, record}
			call, err := BuildCall(binding, request)
			if err != nil {
				return nil, errors.New("Could not build the synthetic Responses request")
			}
			if err := checkCacheToolsRequest(config, call, binding.Session, limit, len(request.Tools)); err != nil {
				return nil, err
			}
			probes[index][groupIndex] = probe{model: languageModel, call: call, sample: CacheToolsSample{Group: group, Phase: phase, Index: index + 1}}
		}
	}
	var samples []CacheToolsSample
	var selectedModel string
	var comparisonIssue error
	for _, pair := range probes {
		for _, probe := range pair {
			if ctx.Err() != nil {
				return samples, ctx.Err()
			}
			sample, responseModel, err := readCacheToolsSample(ctx, probe.model, probe.call, probe.sample)
			if responseModel != "" && selectedModel == "" {
				selectedModel = responseModel
			}
			switch {
			case responseModel == "":
				sample.ResponseModel = "missing"
			case responseModel != selectedModel:
				sample.ResponseModel = "different"
			case responseModel == binding.Config.Model:
				sample.ResponseModel = "requested"
			default:
				sample.ResponseModel = "consistent"
			}
			samples = append(samples, sample)
			if err != nil {
				return samples, fmt.Errorf("%s sample %d: %w", sample.Group, sample.Index, err)
			}
			if comparisonIssue == nil {
				switch {
				case responseModel == "":
					comparisonIssue = errors.New("A Responses result did not report its model")
				case responseModel != selectedModel:
					comparisonIssue = errors.New("The provider reported different models across samples")
				case sample.Input == nil || sample.Cached == nil:
					comparisonIssue = errors.New("A Responses result did not report both input and cached token counts")
				case sample.Fresh == nil:
					comparisonIssue = errors.New("A cached token count exceeded the corresponding input count")
				case sample.Phase == "cold" && *sample.Cached != 0:
					comparisonIssue = errors.New("A cold sample reported cached tokens")
				case sample.Phase == "warm" && *sample.Cached == 0:
					comparisonIssue = errors.New("A warm sample reported no cache hit")
				}
			}
		}
	}
	return samples, comparisonIssue
}

func checkCacheToolsRequest(config transportConfig, call fantasy.Call, session protocol.SessionID, limit int64, tools int) error {
	body, err := requestJSON(config, call)
	if err != nil {
		return errors.New("Could not prepare the synthetic Responses request")
	}
	maxOutput := float64(0)
	switch value := body["max_output_tokens"].(type) {
	case int64:
		maxOutput = float64(value)
	case float64:
		maxOutput = value
	}
	if body["model"] != config.Model || body["prompt_cache_key"] != session.String() || body["store"] != false || body["stream"] != true ||
		maxOutput < 1 || maxOutput > float64(limit) || maxOutput != float64(int64(maxOutput)) || len(wireArray(body["tools"])) != tools ||
		body["prompt"] != nil || body["previous_response_id"] != nil || body["conversation"] != nil || body["background"] == true {
		return errors.New("The channel settings do not preserve the bounded, stateless cache-tools request")
	}
	return nil
}

func readCacheToolsSample(ctx context.Context, languageModel fantasy.LanguageModel, call fantasy.Call, sample CacheToolsSample) (CacheToolsSample, string, error) {
	var responseModel string
	stream, err := languageModel.Stream(context.WithValue(ctx, cacheToolsReceiptKey{}, &responseModel), call)
	if err != nil {
		return sample, "", errors.New("Could not start the Responses request")
	}
	for part := range stream {
		switch part.Type {
		case fantasy.StreamPartTypeFinish, fantasy.StreamPartTypeError:
			usage := PartUsage(part)
			sample.Input, sample.Cached = usage.Input, usage.CacheRead
			if sample.Input != nil && sample.Cached != nil && *sample.Cached <= *sample.Input {
				sample.Fresh = new(*sample.Input - *sample.Cached)
			}
			if part.Type == fantasy.StreamPartTypeFinish {
				return sample, responseModel, nil
			}
			if ctx.Err() != nil {
				return sample, responseModel, ctx.Err()
			}
			if metadata, ok := part.ProviderMetadata[responseMetadataKey].(*responseMetadata); ok && metadata != nil && metadata.Status != nil {
				return sample, responseModel, fmt.Errorf("The Responses request failed (HTTP %d)", *metadata.Status)
			}
			return sample, responseModel, errors.New("The Responses request failed")
		}
	}
	if ctx.Err() != nil {
		return sample, responseModel, ctx.Err()
	}
	return sample, responseModel, errors.New("The Responses request did not finish")
}

func cacheToolsReference() string {
	var text strings.Builder
	for i := 1; i <= 128; i++ {
		fmt.Fprintf(&text, "Reference row %03d records a copper lantern at the north trail, a cedar gate at the return path, and a numbered shelf in the staging room.\n", i)
	}
	return text.String()
}

func cacheToolsDeclarations() ([]Tool, map[string]json.RawMessage) {
	return []Tool{
		{Name: "list_directory", Description: "List files and directories under a workspace-relative directory. Continue a previous page with its cursor."},
		{Name: "read_file", Description: "Read UTF-8 lines from a workspace-relative file. Use the offset and line limit to page large files."},
		{Name: "search_files", Description: "Search workspace text for a literal or regular expression. Restrict the search with path and glob filters and return matching lines."},
	}, map[string]json.RawMessage{
		"list_directory": json.RawMessage(`{"type":"object","properties":{"path":{"type":"string","description":"Workspace-relative directory, or an empty string for the root."},"cursor":{"type":"string","description":"Continuation cursor returned by the previous page."}},"required":["path"],"additionalProperties":false}`),
		"read_file":      json.RawMessage(`{"type":"object","properties":{"path":{"type":"string","description":"Workspace-relative file path."},"offset":{"type":"integer","minimum":0,"description":"Zero-based starting line."},"limit":{"type":"integer","minimum":1,"maximum":2000,"description":"Maximum lines to return."}},"required":["path"],"additionalProperties":false}`),
		"search_files":   json.RawMessage(`{"type":"object","properties":{"query":{"type":"string","minLength":1,"description":"Literal text or a regular expression."},"regex":{"type":"boolean","description":"Interpret the query as a regular expression."},"path":{"type":"string","description":"Workspace-relative path to search."},"globs":{"type":"array","items":{"type":"string"},"description":"File patterns that restrict the search."},"limit":{"type":"integer","minimum":1,"maximum":200,"description":"Maximum matching lines to return."}},"required":["query"],"additionalProperties":false}`),
	}
}
