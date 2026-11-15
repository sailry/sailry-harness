package settings

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestProviderImageGeneration(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name    string
		api     string
		model   preview.ChannelModel
		catalog *preview.ChannelModel
		hosted  bool
		want    string
	}{
		{name: "Responses conversation", api: "responses", model: preview.ChannelModel{Tools: true}, want: viewmodel.GenerateImageTool},
		{name: "Chat Completions standalone", api: "chat_completions", model: preview.ChannelModel{Tools: true}, want: viewmodel.GenerateImage},
		{name: "Gemini native", api: "gemini", model: preview.ChannelModel{Tools: true}, want: viewmodel.GenerateImage},
		{name: "dedicated catalog image", api: "responses", model: preview.ChannelModel{Tools: true}, catalog: &preview.ChannelModel{Generates: []string{viewmodel.GenerateImage}}, want: viewmodel.GenerateImage},
		{name: "dedicated catalog overrides stale hosted choice", api: "responses", model: preview.ChannelModel{Tools: true}, catalog: &preview.ChannelModel{Generates: []string{viewmodel.GenerateImage}}, hosted: true, want: viewmodel.GenerateImage},
		{name: "custom image without tools", api: "responses", model: preview.ChannelModel{CustomCapabilities: true, Generates: []string{viewmodel.GenerateImage}}, want: viewmodel.GenerateImage},
		{name: "custom conversation without catalog", api: "responses", model: preview.ChannelModel{Tools: true, CustomCapabilities: true, Generates: []string{viewmodel.GenerateImage}}, want: viewmodel.GenerateImageTool},
		{name: "remembered hosted on Chat Completions", api: "chat_completions", hosted: true, want: viewmodel.GenerateImageTool},
		{name: "explicit hosted on Chat Completions", api: "chat_completions", model: preview.ChannelModel{Generates: []string{viewmodel.GenerateImageTool}}, want: viewmodel.GenerateImageTool},
	} {
		t.Run(test.name, func(t *testing.T) {
			draft := modelDraft{model: test.model, catalog: test.catalog, hosted: test.hosted}
			if got := draft.imageGeneration(test.api); got != test.want {
				t.Fatalf("image generation = %q; want %q", got, test.want)
			}
		})
	}
}

func TestProviderProtocolSwitchImageGeneration(t *testing.T) {
	t.Parallel()
	editor := providerEditor{
		method: preview.Method{Routes: []preview.Route{
			{ID: "fixture/chat", API: "chat_completions", Custom: true},
			{ID: "fixture/responses", API: "responses", Custom: true},
		}},
		models: draftsOf([]preview.ChannelModel{
			{ID: "conversation", Tools: true, CustomCapabilities: true, Generates: []string{viewmodel.GenerateVideo, viewmodel.GenerateImage, viewmodel.GenerateSpeech}},
			{ID: "native-image", Tools: true, Generates: []string{viewmodel.GenerateImage}},
			{ID: "disabled", Tools: true},
			{ID: "hosted", Generates: []string{viewmodel.GenerateImageTool}},
		}),
	}
	want := [][]string{
		{viewmodel.GenerateVideo, viewmodel.GenerateSpeech, viewmodel.GenerateImageTool},
		{viewmodel.GenerateImage},
		{},
		{viewmodel.GenerateImageTool},
	}
	editor.switchRoute(1)
	for i := range editor.models {
		if got := editor.models[i].model.Generates; !slices.Equal(got, want[i]) {
			t.Fatalf("Responses model %q generation = %v; want %v", editor.models[i].id, got, want[i])
		}
	}
	if !editor.models[0].hosted || !editor.models[0].model.CustomCapabilities {
		t.Fatal("protocol switch lost the custom hosted choice")
	}
	if got := editor.models[1].catalog.Generates; !slices.Equal(got, []string{viewmodel.GenerateImage}) {
		t.Fatalf("protocol switch mutated the catalog: %v", got)
	}
	// Switching back cannot turn a hosted conversation model into an Images
	// API model. The Node reports that capability as unsupported on this API.
	editor.switchRoute(0)
	for i := range editor.models {
		if got := editor.models[i].model.Generates; !slices.Equal(got, want[i]) {
			t.Fatalf("Chat Completions model %q generation = %v; want %v", editor.models[i].id, got, want[i])
		}
	}
}
