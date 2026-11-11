package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"image"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// mediaScript replays fixed tool calls for one conversation model and records
// what each request offered and returned.
type mediaScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	steps   [][][3]string
	offered [][]string
	results map[string]string
}

func (script *mediaScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	var names []string
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.offered = append(script.offered, names)
	step := len(script.offered)
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				switch output := result.Output.(type) {
				case fantasy.ToolResultOutputContentText:
					script.results[result.ToolCallID] = output.Text
				case fantasy.ToolResultOutputContentError:
					script.results[result.ToolCallID] = "error: " + output.Error.Error()
				}
			}
		}
	}
	script.mu.Unlock()
	if step > len(script.steps)+1 {
		return nil, errors.New("unexpected extra media model request")
	}
	var calls [][3]string
	if step <= len(script.steps) {
		calls = script.steps[step-1]
	}
	return func(yield func(fantasy.StreamPart) bool) {
		for _, call := range calls {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: call[0], ToolCallName: call[1], ToolCallInput: call[2]}) {
				return
			}
		}
		if len(calls) == 0 && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// mediaEyes is the assigned vision model: it records the image it was sent.
type mediaEyes struct {
	fantasy.LanguageModel
	mu     sync.Mutex
	images []string
}

func (eyes *mediaEyes) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	eyes.mu.Lock()
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if file, found := fantasy.AsMessagePart[fantasy.FilePart](part); found {
				eyes.images = append(eyes.images, file.MediaType)
			}
		}
	}
	eyes.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "seen", Delta: "A red square"}) {
			input, output := int64(11), int64(3)
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop,
				ProviderMetadata: fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{Reported: true, Input: &input, Output: &output}}})
		}
	}, nil
}

// TestMediaTools proves media routing, a real turn that generates and inspects
// an image through the bundled media package, output confinement, plan mode
// and read-aloud synthesis against a local fake provider.
func TestMediaTools(t *testing.T) {
	t.Run("routing", func(t *testing.T) {
		entry := func(id string, vision bool, generates ...protocol.MediaGeneration) protocol.ConversationModel {
			value := protocol.DefaultConversationModel()
			value.ID, value.Vision, value.Generates = id, vision, generates
			return value
		}
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Api, provider.Authentication = protocol.NewProviderID(), protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey
		provider.Models = []protocol.ConversationModel{
			entry("plain", false), entry("capable", true, protocol.MediaGenerationImage, protocol.MediaGenerationVideo),
			entry("hosted", true, protocol.MediaGenerationImageTool), entry("special", true, protocol.MediaGenerationImage, protocol.MediaGenerationVideo),
		}
		channels := map[protocol.MediaKind]model.MediaChannel{protocol.MediaKindVision: model.MediaChat, protocol.MediaKindImage: model.MediaImages, protocol.MediaKindVideo: model.MediaVideos}
		for _, kind := range []protocol.MediaKind{protocol.MediaKindVision, protocol.MediaKindImage, protocol.MediaKindVideo} {
			for _, test := range []struct {
				name, conversation string
				media              store.MediaCapture
				model              string
				assigned, offered  bool
			}{
				{"assigned model wins over a capable conversation model", "capable", store.MediaCapture{kind: {Model: "special", Provider: &provider}}, "special", true, true},
				{"assigned model serves an incapable conversation model", "plain", store.MediaCapture{kind: {Model: "special", Provider: &provider}}, "special", true, true},
				{"assigned but unavailable is never substituted", "capable", store.MediaCapture{kind: {Model: "special"}}, "", false, false},
				{"assigned model that lost the capability is not offered", "capable", store.MediaCapture{kind: {Model: "plain", Provider: &provider}}, "", false, false},
				{"capable conversation model is used", "capable", nil, "capable", false, true},
				{"another purpose's assignment does not apply", "capable", store.MediaCapture{protocol.MediaKindSpeech: {Model: "special", Provider: &provider}}, "capable", false, true},
				{"incapable conversation model offers nothing", "plain", nil, "", false, false},
			} {
				t.Run(string(kind)+"/"+test.name, func(t *testing.T) {
					inv := &store.AgentInvocation{Provider: &provider, Media: test.media}
					inv.Turn.Config.Model = test.conversation
					route, offered := routeMedia(inv, kind)
					if offered != test.offered {
						t.Fatalf("offered = %v, route = %+v", offered, route)
					}
					if offered && (route.model != test.model || route.assigned != test.assigned || route.channel != channels[kind]) {
						t.Fatalf("route = %s assigned %v through %s", route.model, route.assigned, route.channel)
					}
				})
			}
		}
		t.Run("image/hosted tool conversation model", func(t *testing.T) {
			inv := &store.AgentInvocation{Provider: &provider}
			inv.Turn.Config.Model = "hosted"
			if route, offered := routeMedia(inv, protocol.MediaKindImage); !offered || route.channel != model.MediaImageTool || route.assigned {
				t.Fatalf("hosted image route = %+v, %v", route, offered)
			}
			if _, offered := routeMedia(inv, protocol.MediaKindVideo); offered {
				t.Fatal("hosted image model was offered for video")
			}
			if _, offered := routeMedia(&store.AgentInvocation{}, protocol.MediaKindImage); offered {
				t.Fatal("a turn without a provider was offered media")
			}
		})
	})

	var picture bytes.Buffer
	if err := png.Encode(&picture, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	var mu sync.Mutex
	requests := map[string][]map[string]any{}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(request.Body)
		var input map[string]any
		_ = json.Unmarshal(body, &input)
		mu.Lock()
		requests[request.URL.Path] = append(requests[request.URL.Path], input)
		mu.Unlock()
		if request.Method != http.MethodPost || request.Header.Get("Authorization") != "Bearer media-key" {
			http.Error(writer, "unexpected request", http.StatusBadRequest)
			return
		}
		switch request.URL.Path {
		case "/v1/images/generations":
			writer.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(writer).Encode(map[string]any{"data": []any{map[string]any{"b64_json": base64.StdEncoding.EncodeToString(picture.Bytes())}}, "usage": map[string]any{"input_tokens": 7, "output_tokens": 5, "total_tokens": 12}})
		case "/v1/audio/speech":
			writer.Header().Set("Content-Type", "audio/mpeg")
			_, _ = writer.Write([]byte("ID3 spoken"))
		default:
			http.NotFound(writer, request)
		}
	}))
	defer server.Close()
	seen := func(path string) []map[string]any {
		mu.Lock()
		defer mu.Unlock()
		return slices.Clone(requests[path])
	}

	tool := func(name string) string { return operations.Alias("media", name) }
	discover := [3]string{"discover", "tool_search", `{"query":"select:` + strings.Join([]string{tool("generate_image"), tool("inspect_image"), tool("generate_video")}, ",") + `"}`}
	painter := &mediaScript{results: map[string]string{}, steps: [][][3]string{
		{discover},
		{{"paint", tool("generate_image"), `{"prompt":"A red square","file_name":"square.png"}`}},
		{{"look", tool("inspect_image"), `{"prompt":"What is this?","path":"assets/generated/square.png"}`}},
	}}
	planner := &mediaScript{results: map[string]string{}, steps: [][][3]string{
		{discover},
		{{"paint", tool("generate_image"), `{"prompt":"A blue circle","file_name":"circle.png"}`}},
	}}
	eyes := &mediaEyes{}
	var bound []string
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
			mu.Lock()
			bound = append(bound, binding.Config.Model)
			mu.Unlock()
			switch binding.Config.Model {
			case "plain":
				return painter, nil
			case "planner":
				return planner, nil
			case "eyes":
				return eyes, nil
			}
			return nil, errors.New("unexpected media model " + binding.Config.Model)
		},
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("media tools must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	// The bundled media package is installed and enabled by default.
	secret := protocol.NewSecret("media-key")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Media fixture", true
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, server.URL+"/v1", "plain"
	for _, id := range []string{"plain", "planner", "eyes", "painter", "voice"} {
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Context, entry.Output, entry.Tools, entry.CustomCapabilities = id, 32000, 8000, true, true
		switch id {
		case "eyes":
			entry.Vision = true
		case "painter":
			entry.Generates = []protocol.MediaGeneration{protocol.MediaGenerationImage}
		case "voice":
			entry.Generates = []protocol.MediaGeneration{protocol.MediaGenerationSpeech}
		}
		provider.Models = append(provider.Models, entry)
	}
	commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	assign := func(revision uint64, bindings map[protocol.MediaKind]string) protocol.MediaSettings {
		t.Helper()
		settings := protocol.MediaSettings{Revision: revision, Bindings: map[protocol.MediaKind]*protocol.MediaBinding{}}
		for kind, name := range bindings {
			settings.Bindings[kind] = &protocol.MediaBinding{Provider: provider.ID, Model: name}
		}
		return commandFixture(t, node, protocol.Command{Kind: "save_media_settings", Data: settings}).Data.(protocol.MediaSettings)
	}
	settings := assign(0, map[protocol.MediaKind]string{protocol.MediaKindVision: "eyes", protocol.MediaKindImage: "painter", protocol.MediaKindSpeech: "voice"})
	if len(settings.Unavailable) != 0 {
		t.Fatalf("fresh assignments are unavailable: %+v", settings)
	}

	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Media tools", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	run := func(t *testing.T, name string, mode protocol.WorkMode) (protocol.Session, protocol.ConversationHistory) {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, name, mode, protocol.PermissionFull
		session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
		commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Make a picture"},
		}})
		var history protocol.ConversationHistory
		for completed := false; !completed; {
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, run := range history.Page.Runs {
				switch run.Status {
				case protocol.ConversationStatusCompleted:
					completed = true
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("media turn failed: %+v", run)
				}
			}
			if !completed {
				if _, err := subscription.Next(t.Context()); err != nil {
					t.Fatal(err)
				}
			}
		}
		return session, history
	}

	var session protocol.Session
	t.Run("turn generates and inspects an image", func(t *testing.T) {
		var history protocol.ConversationHistory
		session, history = run(t, "plain", protocol.WorkModeCode)
		painter.mu.Lock()
		offered, results := painter.offered, painter.results
		painter.mu.Unlock()
		if len(offered) != 4 {
			t.Fatalf("media turn made %d model requests: %v", len(offered), results)
		}
		if !slices.Contains(offered[1], tool("generate_image")) || !slices.Contains(offered[1], tool("inspect_image")) {
			t.Fatalf("assigned media tools were not offered: %v", offered[1])
		}
		// No video model is assigned and the conversation model cannot
		// generate video, so the declared tool does not exist for this turn.
		for _, names := range offered {
			if slices.Contains(names, tool("generate_video")) {
				t.Fatalf("unrouted video tool was offered: %v", names)
			}
		}
		saved, err := os.ReadFile(filepath.Join(root, "assets", "generated", "square.png"))
		if err != nil || !bytes.Equal(saved, picture.Bytes()) {
			t.Fatalf("generated image was not written to the worktree: %d bytes, %v", len(saved), err)
		}
		calls := seen("/v1/images/generations")
		if len(calls) != 1 || calls[0]["model"] != "painter" || calls[0]["prompt"] != "A red square" {
			t.Fatalf("images endpoint requests = %v", calls)
		}
		for _, fragment := range []string{"assets/generated/square.png", "painter"} {
			if !strings.Contains(results["paint"], fragment) {
				t.Fatalf("generation result %q omits %s", results["paint"], fragment)
			}
		}
		if strings.Contains(results["paint"], base64.StdEncoding.EncodeToString(picture.Bytes())) {
			t.Fatal("generated bytes were replayed to the model")
		}
		// The non-vision conversation model asked the assigned vision model.
		if !strings.Contains(results["look"], "A red square") || !strings.Contains(results["look"], "eyes") {
			t.Fatalf("inspection result = %q", results["look"])
		}
		eyes.mu.Lock()
		images := slices.Clone(eyes.images)
		eyes.mu.Unlock()
		mu.Lock()
		models := slices.Clone(bound)
		mu.Unlock()
		if len(images) != 1 || images[0] != "image/png" || !slices.Contains(models, "eyes") || slices.Contains(models, "painter") {
			t.Fatalf("vision model received %v; chat bindings %v", images, models)
		}
		// The generated image is recorded with its tool result for display.
		shown := 0
		for _, entry := range history.Page.Entries {
			body, err := json.Marshal(entry)
			if err != nil {
				t.Fatal(err)
			}
			var walk func(any)
			walk = func(value any) {
				switch value := value.(type) {
				case map[string]any:
					if images, ok := value["images"].([]any); ok && value["name"] == tool("generate_image") {
						shown += len(images)
					}
					for _, child := range value {
						walk(child)
					}
				case []any:
					for _, child := range value {
						walk(child)
					}
				}
			}
			var projected any
			if err := json.Unmarshal(body, &projected); err != nil {
				t.Fatal(err)
			}
			walk(projected)
		}
		if shown != 1 {
			t.Fatalf("generated image presentations = %d", shown)
		}
		// Usage names the provider and model that actually ran.
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs, query.Dimension = time.Now().Add(-time.Hour).UnixMilli(), time.Now().Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
		report := commandFixture(t, node, protocol.Command{Kind: "read_usage", Data: query}).Data.(protocol.UsageReport)
		usage := map[string]protocol.ConversationUsage{}
		for _, item := range report.Requests.Items {
			if item.Session == session.ID && item.Provider == provider.ID {
				usage[item.Model] = item.Tokens
			}
		}
		if usage["painter"].Input != 7 || usage["painter"].Output != 5 || usage["eyes"].Input != 11 || usage["eyes"].Output != 3 {
			t.Fatalf("media usage = %+v", report.Requests.Items)
		}
	})

	t.Run("output path stays inside the worktree", func(t *testing.T) {
		inv := &store.AgentInvocation{Session: session}
		inv.Worktree.Path = root
		operation := protocol.ToolOperationGenerateImage
		generate := node.mediaTool(inv, protocol.PluginReference{}, tool("generate_image"), operation, protocol.MediaKindImage, mediaRoute{}, nil)
		before := len(seen("/v1/images/generations"))
		for path, code := range map[string]protocol.ErrorCode{
			"../escape.png": "", "/tmp/escape.png": "", "assets/../../escape.png": "", "assets/generated/note.svg": protocol.ErrorCodeInvalidRequest,
			"assets/generated/square.png": protocol.ErrorCodeConflict,
		} {
			arguments, err := json.Marshal(map[string]string{"prompt": "A red square", "path": path})
			if err != nil {
				t.Fatal(err)
			}
			err = generate.Prepare(t.Context(), canonical.ToolRequest{ID: "confined", Arguments: arguments})
			var fault *protocol.Fault
			if err == nil || code != "" && (!errors.As(err, &fault) || fault.Code != code) {
				t.Fatalf("output path %q was not refused with %q: %v", path, code, err)
			}
		}
		if err := generate.Prepare(t.Context(), canonical.ToolRequest{ID: "confined", Arguments: json.RawMessage(`{"prompt":"A red square","path":"assets/generated/other.png"}`)}); err != nil {
			t.Fatalf("confined output path was refused: %v", err)
		}
		if _, err := os.Stat(filepath.Join(filepath.Dir(root), "escape.png")); !errors.Is(err, os.ErrNotExist) || len(seen("/v1/images/generations")) != before {
			t.Fatalf("refused output reached the provider or the disk: %v", err)
		}
	})

	t.Run("plan mode refuses generation", func(t *testing.T) {
		before := len(seen("/v1/images/generations"))
		run(t, "planner", protocol.WorkModePlan)
		planner.mu.Lock()
		offered, results := planner.offered, planner.results
		planner.mu.Unlock()
		if len(offered) != 3 || !strings.Contains(results["paint"], "planning mode permits read-only tools") {
			t.Fatalf("plan turn requests = %d, generation result = %q", len(offered), results["paint"])
		}
		if _, err := os.Stat(filepath.Join(root, "assets", "generated", "circle.png")); !errors.Is(err, os.ErrNotExist) || len(seen("/v1/images/generations")) != before {
			t.Fatalf("plan mode generated an image: %v", err)
		}
	})

	t.Run("speech synthesis", func(t *testing.T) {
		speak := func(text string, voice *string) (protocol.Output, *protocol.Fault) {
			value := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "synthesize_speech", Data: protocol.CommandSynthesizeSpeechPayload{Text: text, Voice: voice}}))
			return value.Output, value.Fault
		}
		output, fault := speak("Read this aloud", nil)
		if fault != nil {
			t.Fatal(fault)
		}
		audio := output.Data.(protocol.SpeechAudio)
		data, err := base64.StdEncoding.DecodeString(audio.DataBase64)
		if err != nil || string(data) != "ID3 spoken" || audio.MediaType != "audio/mpeg" || audio.Provider != provider.ID || audio.Model != "voice" {
			t.Fatalf("speech audio = %+v, %q, %v", audio, data, err)
		}
		calls := seen("/v1/audio/speech")
		if len(calls) != 1 || calls[0]["model"] != "voice" || calls[0]["input"] != "Read this aloud" || calls[0]["voice"] != "alloy" || calls[0]["response_format"] != "mp3" {
			t.Fatalf("speech requests = %v", calls)
		}
		if _, fault := speak("  ", nil); fault == nil || fault.Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("empty speech text fault = %v", fault)
		}
		if _, fault := speak(strings.Repeat("a", model.MaxSpeechCharacters+1), nil); fault == nil || fault.Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("oversized speech text fault = %v", fault)
		}
		// The assigned model losing its provider is reported, not replaced.
		var disabled protocol.ConversationProvider
		for _, current := range commandFixture(t, node, protocol.Command{Kind: "list_providers"}).Data.([]protocol.ConversationProvider) {
			if current.ID == provider.ID {
				disabled = current
			}
		}
		disabled.Enabled = false
		commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: disabled, ExpectedRevision: disabled.Revision}})
		read := commandFixture(t, node, protocol.Command{Kind: "read_media_settings"}).Data.(protocol.MediaSettings)
		if !slices.Contains(read.Unavailable, protocol.MediaKindSpeech) || len(read.Bindings) != 3 {
			t.Fatalf("disabled provider left media settings %+v", read)
		}
		if _, fault := speak("Read this aloud", nil); fault == nil || fault.Code != protocol.ErrorCodeNotConfigured {
			t.Fatalf("unavailable speech model fault = %v", fault)
		}
		assign(read.Revision, map[protocol.MediaKind]string{})
		if _, fault := speak("Read this aloud", nil); fault == nil || fault.Code != protocol.ErrorCodeNotConfigured || len(seen("/v1/audio/speech")) != 1 {
			t.Fatalf("unassigned speech model fault = %v", fault)
		}
	})
}
