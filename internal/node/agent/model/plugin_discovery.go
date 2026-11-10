package model

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func DiscoverPlugin(ctx context.Context, binding Binding, capture PluginCapture) (protocol.ConversationDiscoveryCatalog, error) {
	var catalog protocol.ConversationDiscoveryCatalog
	languageModel, err := NewPlugin(ctx, binding, capture)
	if err != nil {
		return catalog, err
	}
	model := languageModel.(*pluginModel)
	if !model.capture.Declaration.Discovery {
		return catalog, &Failure{Kind: NotConfigured, Message: "channel plugin does not declare model discovery"}
	}
	owner, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	worker, err := plugins.NewTranslation(owner, model.capture.Bundle)
	if err != nil {
		return catalog, pluginTranslationFailure(err)
	}
	defer worker.Close()
	raw, err := worker.Call("prepare_discovery", map[string]string{"endpoint": binding.Provider.Endpoint}, pluginDiscoveryBytes)
	if err != nil {
		return catalog, pluginTranslationFailure(err)
	}
	var prepared protocol.PluginModelAdapterWirePrepared
	if json.Unmarshal(raw, &prepared) != nil {
		return catalog, &Failure{Kind: InvalidRequest, Message: "model translation returned an invalid discovery request"}
	}
	response, err := model.send(owner, prepared, pluginDiscoveryBytes)
	if err != nil {
		return catalog, err
	}
	defer response.Body.Close()
	headers, err := model.publicHeaders(response.Header)
	if err != nil {
		return catalog, err
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, pluginDiscoveryBytes+1))
	if err != nil {
		return catalog, &Failure{Kind: Network, Message: "model connection failed"}
	}
	if len(body) > pluginDiscoveryBytes {
		return catalog, pluginResourceFailure()
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return catalog, &Failure{Kind: Unavailable, Message: fmt.Sprintf("model discovery failed with HTTP %d", response.StatusCode)}
	}
	if !utf8.Valid(body) {
		return catalog, &Failure{Kind: InvalidRequest, Message: "model discovery response is not UTF-8"}
	}
	if err := model.public(string(body)); err != nil {
		return catalog, err
	}
	raw, err = worker.Call("decode_discovery", map[string]any{"endpoint": binding.Provider.Endpoint, "status": response.StatusCode, "headers": headers, "body": string(body)}, pluginDiscoveryBytes)
	if err != nil {
		return catalog, pluginTranslationFailure(err)
	}
	if json.Unmarshal(raw, &catalog) != nil {
		return catalog, &Failure{Kind: InvalidRequest, Message: "model translation returned an invalid discovery catalog"}
	}
	if err := model.public(string(raw)); err != nil {
		return catalog, err
	}
	if strings.TrimRight(catalog.Endpoint, "/") != strings.TrimRight(binding.Provider.Endpoint, "/") {
		return catalog, &Failure{Kind: InvalidRequest, Message: "model discovery returned a different endpoint"}
	}
	seen := map[string]bool{}
	for index := range catalog.Models {
		entry := &catalog.Models[index]
		if strings.TrimSpace(entry.ID) == "" || strings.ContainsFunc(entry.ID, unicode.IsControl) || seen[entry.ID] ||
			entry.Context != nil && *entry.Context == 0 || entry.Output != nil && *entry.Output == 0 ||
			entry.Context != nil && entry.Output != nil && *entry.Output > *entry.Context {
			return catalog, &Failure{Kind: InvalidRequest, Message: "model discovery returned invalid metadata"}
		}
		seen[entry.ID] = true
		if entry.Capabilities == nil {
			value := protocol.DefaultConversationDiscoveryCapabilities()
			entry.Capabilities = &value
		}
		if err := providers.ValidateFeatures(protocol.ConversationModelApiPlugin, protocol.ConversationModel{Capabilities: entry.Capabilities}, nil); err != nil {
			return catalog, &Failure{Kind: InvalidRequest, Message: err.Error()}
		}
		disabled := false
		if !model.capture.Declaration.Capabilities.Vision {
			value := disabled
			entry.Capabilities.Vision = &value
		}
		if !model.capture.Declaration.Capabilities.Tools {
			value := disabled
			entry.Capabilities.Tools = &value
		}
		if !model.capture.Declaration.Capabilities.Reasoning {
			value := disabled
			entry.Capabilities.Reasoning = &value
		}
	}
	return catalog, nil
}
