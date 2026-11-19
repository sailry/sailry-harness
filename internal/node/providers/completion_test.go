package providers

import (
	"reflect"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCompleteChannelModelInputs(t *testing.T) {
	const endpoint = "https://generativelanguage.googleapis.com/v1beta"
	all := []string{"text", "image", "video", "audio", "pdf"}
	for _, row := range []struct {
		name, endpoint, referenceID string
		catalog, manual, want       []string
		native                      *bool
		custom, wantVision          bool
	}{
		{name: "exact offering", endpoint: endpoint, catalog: all, want: all, wantVision: true},
		{name: "different model", endpoint: endpoint, referenceID: "other", catalog: all, wantVision: true},
		{name: "unlisted proxy", endpoint: "https://proxy.invalid/v1", catalog: all, wantVision: true},
		{name: "explicit empty catalog list", endpoint: endpoint, catalog: []string{}, want: []string{}},
		{name: "native or plugin denies image", endpoint: endpoint, catalog: all, native: new(false), want: []string{"text", "video", "audio", "pdf"}},
		{name: "native grants just image", endpoint: endpoint, catalog: []string{"pdf"}, native: new(true), want: []string{"pdf", "image"}, wantVision: true},
		{name: "native vision without catalog inputs", endpoint: endpoint, native: new(true), wantVision: true},
		{name: "manual clear", endpoint: endpoint, catalog: all, native: new(true), custom: true, manual: []string{}, want: []string{}},
		{name: "legacy manual image", endpoint: endpoint, catalog: all, native: new(false), custom: true, wantVision: true},
	} {
		t.Run(row.name, func(t *testing.T) {
			provider := protocol.ConversationProvider{Api: protocol.ConversationModelApiGemini, Endpoint: row.endpoint, Authentication: protocol.AuthenticationApiKey}
			model := protocol.DefaultConversationModel()
			model.ID, model.Context, model.Output = "fixture", 4096, 1024
			model.CustomCapabilities, model.Vision = row.custom, true
			model.Inputs = []string{"audio"} // Previous automatic data cannot survive a missing match.
			if row.custom {
				model.Inputs = row.manual
			}
			if row.native != nil {
				model.Capabilities = &protocol.ConversationDiscoveryCapabilities{Vision: row.native}
			}
			reference := protocol.ConversationCatalogModel{ID: model.ID, Inputs: slices.Clone(row.catalog), Tools: new(false), Reasoning: new(false)}
			if row.referenceID != "" {
				reference.ID = row.referenceID
			}
			got := CompleteChannelModel(provider, model, &reference)
			if !reflect.DeepEqual(got.Inputs, row.want) || got.Vision != row.wantVision || got.AcceptsInput("image") != row.wantVision {
				t.Fatalf("inputs = %#v (vision %v), want %#v (vision %v)", got.Inputs, got.Vision, row.want, row.wantVision)
			}
			if !reflect.DeepEqual(reference.Inputs, row.catalog) {
				t.Fatalf("completion mutated the directory inputs: %#v", reference.Inputs)
			}
		})
	}
}
