package model

import (
	"encoding/base64"
	"encoding/json"
	"fmt"

	"charm.land/fantasy"
	"charm.land/fantasy/providers/anthropic"
	"charm.land/fantasy/providers/google"
	"charm.land/fantasy/providers/openai"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

const indexMetadataKey = "sailry.index"

type indexMetadata struct{ Index uint64 }

func (*indexMetadata) Options() {}
func PartIndex(part fantasy.StreamPart) (uint64, bool) {
	value, ok := part.ProviderMetadata[indexMetadataKey].(*indexMetadata)
	if !ok || value == nil {
		return 0, false
	}
	return value.Index, true
}

// ApplyStreamMetadata is the provider-to-canonical carrier boundary. The engine
// owns text accumulation and slot identity; transport owns provider signatures.
func ApplyStreamMetadata(block *canonical.Block, part fantasy.StreamPart) error {
	if block == nil {
		return fmt.Errorf("canonical output block is missing")
	}
	if err := ApplyPluginMetadata(block, part); err != nil {
		return err
	}
	for key, raw := range part.ProviderMetadata {
		switch key {
		case anthropic.Name:
			value, ok := raw.(*anthropic.ReasoningOptionMetadata)
			if !ok || value == nil {
				continue
			}
			if value.RedactedData != "" {
				block.Kind = "redacted_thinking"
				block.Data = &canonical.RedactedThinking{Data: []string{value.RedactedData}, Before: []*uint64{nil}}
				continue
			}
			if value.Signature != "" {
				thinking := canonical.Thinking{}
				if block.Kind == "thinking" && block.Data != nil {
					if err := decodeBlock(block.Data, &thinking); err != nil {
						return err
					}
				}
				signature := value.Signature
				thinking.Signature = &signature
				block.Kind = "thinking"
				block.Data = &thinking
			}
		case openai.Name:
			value, ok := raw.(*openai.ResponsesReasoningMetadata)
			if !ok || value == nil {
				continue
			}
			thinking := canonical.Thinking{}
			if block.Kind == "thinking" && block.Data != nil {
				if err := decodeBlock(block.Data, &thinking); err != nil {
					return err
				}
			}
			if value.EncryptedContent != nil {
				signature := *value.EncryptedContent
				thinking.Signature = &signature
			}
			if thinking.Text == "" {
				for i, text := range value.Summary {
					if i > 0 {
						thinking.Text += "\n"
					}
					thinking.Text += text
				}
			}
			block.Kind = "thinking"
			block.Data = &thinking
		case google.Name:
			value, ok := raw.(*google.ReasoningMetadata)
			if !ok || value == nil || value.Signature == "" {
				continue
			}
			signature := base64.StdEncoding.EncodeToString([]byte(value.Signature))
			if block.Kind == "tool_request" {
				var call canonical.ToolRequest
				if err := decodeBlock(block.Data, &call); err != nil {
					return err
				}
				if value.ToolID != "" && value.ToolID != call.ID {
					return fmt.Errorf("tool thought signature identity does not match")
				}
				fields := map[string]json.RawMessage{}
				if call.Provider != nil {
					for key, value := range *call.Provider {
						fields[key] = value
					}
				}
				fields["thoughtSignature"], _ = json.Marshal(signature)
				call.Provider = &fields
				block.Data = &call
			} else {
				thinking := canonical.Thinking{}
				if block.Kind == "thinking" && block.Data != nil {
					if err := decodeBlock(block.Data, &thinking); err != nil {
						return err
					}
				}
				thinking.Signature = &signature
				block.Kind = "thinking"
				block.Data = &thinking
			}
		}
	}
	return nil
}
