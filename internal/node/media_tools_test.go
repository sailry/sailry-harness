package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
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
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
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

// TestMediaTools proves media routing, the withdrawal of the bundled media
// package from an existing profile, a real turn that generates and inspects
// an image with the Node's own tools, a turn whose models offer none, output
// confinement, plan mode and read-aloud synthesis against a local fake
// provider.
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
	movie := append([]byte{0, 0, 0, 24}, []byte("ftypmp42 fixture video")...)
	var mu sync.Mutex
	requests := map[string][]map[string]any{}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(request.Body)
		var input map[string]any
		_ = json.Unmarshal(body, &input)
		mu.Lock()
		requests[request.URL.Path] = append(requests[request.URL.Path], input)
		mu.Unlock()
		video := strings.HasPrefix(request.URL.Path, "/v1/videos")
		if request.Method != http.MethodPost && !video || request.Header.Get("Authorization") != "Bearer media-key" {
			http.Error(writer, "unexpected request", http.StatusBadRequest)
			return
		}
		// Each video submission starts a job. The provider ends the first
		// without a video; the second is lost to one failed read and is
		// complete on every later one.
		mu.Lock()
		submitted, reads := len(requests["/v1/videos"]), len(requests[request.URL.Path])
		mu.Unlock()
		switch request.URL.Path {
		case "/v1/videos":
			_, _ = fmt.Fprintf(writer, `{"id":"video_%d","status":"queued"}`, submitted)
		case "/v1/videos/video_1":
			_, _ = writer.Write([]byte(`{"id":"video_1","status":"failed","error":{"message":"content policy"}}`))
		case "/v1/videos/video_2":
			if reads == 1 {
				http.Error(writer, "try later", http.StatusServiceUnavailable)
				return
			}
			_, _ = writer.Write([]byte(`{"id":"video_2","status":"completed","usage":{"input_tokens":4,"output_tokens":0}}`))
		case "/v1/videos/video_2/content":
			_, _ = writer.Write(movie)
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

	tool := func(name string) string { return name }
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
	bare := &mediaScript{results: map[string]string{}, steps: [][][3]string{{discover}}}
	asker := &mediaScript{results: map[string]string{}, steps: [][][3]string{
		{discover},
		{{"first", tool("generate_image"), `{"prompt":"A green triangle","file_name":"first.png"}`}},
		{{"second", tool("generate_image"), `{"prompt":"A green triangle","file_name":"second.png"}`}},
	}}
	// filmer generates video itself: the provider ends its first job, loses
	// sight of the second, and the third call names that job.
	filmer := &mediaScript{results: map[string]string{}, steps: [][][3]string{
		{discover},
		{{"ended", tool("generate_video"), `{"prompt":"A wave","file_name":"wave.mp4"}`}},
		{{"lost", tool("generate_video"), `{"prompt":"A wave","file_name":"wave.mp4"}`}},
		{{"fetched", tool("generate_video"), `{"prompt":"A wave","file_name":"wave.mp4","job":"video_2"}`}},
	}}
	// twin asks for the same output file twice in one step.
	twin := &mediaScript{results: map[string]string{}, steps: [][][3]string{
		{discover},
		{{"one", tool("generate_image"), `{"prompt":"A twin","file_name":"twin.png"}`}, {"two", tool("generate_image"), `{"prompt":"A twin","file_name":"twin.png"}`}},
	}}
	eyes := &mediaEyes{}
	var bound []string
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	profile := filepath.Join(t.TempDir(), "profile")
	options := Options{
		Environment: &environment, Model: func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
			mu.Lock()
			bound = append(bound, binding.Config.Model)
			mu.Unlock()
			switch binding.Config.Model {
			case "plain":
				return painter, nil
			case "planner":
				return planner, nil
			case "bare":
				return bare, nil
			case "asker":
				return asker, nil
			case "filmer":
				return filmer, nil
			case "twin":
				return twin, nil
			case "eyes":
				return eyes, nil
			}
			return nil, errors.New("unexpected media model " + binding.Config.Model)
		},
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("media tools must not launch a process")
		}},
	}
	// An existing profile holds the bundled media package, enabled, with its
	// settings page and its three tools.
	installed := func(node *Node) []string {
		var names []string
		for _, summary := range snapshotFixture(t, node).Plugins {
			if summary.Name == "media" {
				names = append(names, summary.Name)
				continue
			}
			info := commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: &protocol.CommandReadPluginPayload{Name: summary.Name}}).Data.(protocol.PluginInfo)
			if info.Extension == nil {
				continue
			}
			for _, declaration := range info.Extension.Tools {
				if declaration.Handler != nil && declaration.Handler.Operation != nil {
					if _, media := mediaOperationKind(*declaration.Handler.Operation); media {
						names = append(names, summary.Name+"/"+declaration.Name)
					}
				}
			}
		}
		return names
	}
	earlier, err := StartWithOptions(t.Context(), profile, options)
	if err != nil {
		t.Fatal(err)
	}
	if found := installed(earlier); len(found) != 0 {
		t.Fatalf("a fresh profile holds a media package or a package media tool: %v", found)
	}
	fixture, err := filepath.Abs(filepath.Join("testdata", "media-0.1.4"))
	if err != nil {
		t.Fatal(err)
	}
	retired, err := earlier.plugins.manager.Install(t.Context(), fixture, "", "media")
	if err != nil || retired.Extension == nil || retired.Extension.SettingsPage == nil || len(retired.Extension.Tools) != 3 {
		t.Fatalf("media 0.1.4 fixture: %#v %v", retired.Issues, err)
	}
	retired.Origin = &protocol.PluginOrigin{Kind: "bundled"}
	if err := earlier.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, err := store.InstallPlugin(t.Context(), db, retired, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if found := installed(earlier); !slices.Equal(found, []string{"media"}) {
		t.Fatalf("the existing profile's media package: %v", found)
	}
	if err := earlier.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	// The same profile starts without the package: it contributes no
	// settings page and no tool, and it is not installed again.
	node, err := StartWithOptions(t.Context(), profile, options)
	if err != nil {
		t.Fatalf("a profile holding the withdrawn media package did not start: %v", err)
	}
	defer node.Stop(context.Background())
	if found := installed(node); len(found) != 0 {
		t.Fatalf("the withdrawn media package still contributes: %v", found)
	}
	var recorded bool
	var captured protocol.PluginInfo
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		if recorded, err = store.PluginRecorded(t.Context(), db, "media"); err != nil {
			return err
		}
		// A turn that captured the package still resolves its version.
		captured, err = store.ReadPluginVersion(t.Context(), db, protocol.PluginReference{Name: "media", Digest: retired.Summary.Digest})
		return err
	}); err != nil || !recorded || captured.Extension == nil || len(captured.Extension.Tools) != 3 {
		t.Fatalf("the withdrawal lost the removal record or the captured version: recorded %v, %v", recorded, err)
	}
	secret := protocol.NewSecret("media-key")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Media fixture", true
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, server.URL+"/v1", "plain"
	for _, id := range []string{"plain", "planner", "bare", "asker", "filmer", "twin", "eyes", "painter", "voice"} {
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Context, entry.Output, entry.Tools, entry.CustomCapabilities = id, 32000, 8000, true, true
		switch id {
		case "eyes":
			entry.Vision = true
		case "painter":
			entry.Generates = []protocol.MediaGeneration{protocol.MediaGenerationImage}
		case "filmer":
			entry.Generates = []protocol.MediaGeneration{protocol.MediaGenerationVideo}
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
	// run completes one turn. With a decision the turn asks for approval and
	// every prompt is answered with it; prompts counts them.
	prompts := 0
	run := func(t *testing.T, name string, mode protocol.WorkMode, decision ...string) (protocol.Session, protocol.ConversationHistory) {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, name, mode, protocol.PermissionFull
		if len(decision) != 0 {
			config.Permission = protocol.PermissionAsk
		}
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
			for _, approval := range history.Page.Approvals {
				if approval.State == protocol.ConversationApprovalStatePending && len(decision) != 0 {
					if !approval.CanRemember {
						t.Fatalf("media approval cannot be remembered: %+v", approval)
					}
					prompts++
					commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: decision[0]}}})
				}
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
		specs, err := operations.MediaSpecs()
		if err != nil {
			t.Fatal(err)
		}
		var generate engine.Tool
		for _, spec := range specs {
			if spec.Name == "generate_image" {
				if generate, err = node.mediaTool(inv, spec, protocol.MediaKindImage, mediaRoute{}); err != nil {
					t.Fatal(err)
				}
			}
		}
		before := len(seen("/v1/images/generations"))
		for arguments, code := range map[string]protocol.ErrorCode{
			`{"prompt":"A red square","file_name":"../escape.png"}`:       protocol.ErrorCodeInvalidRequest,
			`{"prompt":"A red square","file_name":"/tmp/escape.png"}`:     protocol.ErrorCodeInvalidRequest,
			`{"prompt":"A red square","file_name":"nested\\escape.png"}`:  protocol.ErrorCodeInvalidRequest,
			`{"prompt":"A red square","file_name":"note.svg"}`:            protocol.ErrorCodeInvalidRequest,
			`{"prompt":"A red square","path":"../escape.png"}`:            protocol.ErrorCodeInvalidRequest,
			`{"prompt":"A red square","file_name":"other.png","extra":1}`: protocol.ErrorCodeInvalidRequest,
			`{"prompt":"  ","file_name":"other.png"}`:                     protocol.ErrorCodeInvalidRequest,
			`{"prompt":"A red square","file_name":"square.png"}`:          protocol.ErrorCodeConflict,
		} {
			err := generate.Prepare(t.Context(), canonical.ToolRequest{ID: "confined", Arguments: json.RawMessage(arguments)})
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != code {
				t.Fatalf("arguments %s were not refused with %q: %v", arguments, code, err)
			}
		}
		if err := generate.Prepare(t.Context(), canonical.ToolRequest{ID: "confined", Arguments: json.RawMessage(`{"prompt":"A red square","file_name":"other.png"}`)}); err != nil {
			t.Fatalf("confined output path was refused: %v", err)
		}
		if err := generate.Prepare(t.Context(), canonical.ToolRequest{ID: "unnamed", Arguments: json.RawMessage(`{"prompt":"A red square"}`)}); err != nil {
			t.Fatalf("generation without a file name was refused: %v", err)
		}
		if _, err := os.Stat(filepath.Join(filepath.Dir(root), "escape.png")); !errors.Is(err, os.ErrNotExist) || len(seen("/v1/images/generations")) != before {
			t.Fatalf("refused output reached the provider or the disk: %v", err)
		}
	})

	t.Run("generation asks and a session approval is remembered", func(t *testing.T) {
		before := len(seen("/v1/images/generations"))
		run(t, "asker", protocol.WorkModeCode, "approve_session")
		asker.mu.Lock()
		results := asker.results
		asker.mu.Unlock()
		for _, name := range []string{"first.png", "second.png"} {
			if _, err := os.Stat(filepath.Join(root, "assets", "generated", name)); err != nil {
				t.Fatalf("approved generation did not write %s: %v (%v)", name, err, results)
			}
		}
		if prompts != 1 || len(seen("/v1/images/generations")) != before+2 {
			t.Fatalf("two generations asked %d times and reached the provider %d times", prompts, len(seen("/v1/images/generations"))-before)
		}
	})

	t.Run("the output file is taken before the provider is paid", func(t *testing.T) {
		before := len(seen("/v1/images/generations"))
		run(t, "twin", protocol.WorkModeCode)
		twin.mu.Lock()
		results := twin.results
		twin.mu.Unlock()
		if _, err := os.Stat(filepath.Join(root, "assets", "generated", "twin.png")); err != nil || !strings.Contains(results["one"], "assets/generated/twin.png") {
			t.Fatalf("the first of two calls for one file did not save it: %v (%v)", err, results)
		}
		if paid := len(seen("/v1/images/generations")) - before; paid != 1 || !strings.Contains(results["two"], "a file already exists at the media output path") {
			t.Fatalf("two calls for one file reached the provider %d times: %q", paid, results["two"])
		}
	})

	t.Run("a video job is ended, lost and fetched without paying twice", func(t *testing.T) {
		node.mediaPoll = time.Millisecond
		session, _ := run(t, "filmer", protocol.WorkModeCode)
		filmer.mu.Lock()
		results := filmer.results
		filmer.mu.Unlock()
		// A job the provider ended is a definite error: nothing is left to fetch.
		if !strings.Contains(results["ended"], "video job video_1 produced no video") || !strings.Contains(results["ended"], "content policy") || strings.Contains(results["ended"], `"job"`) {
			t.Fatalf("a job the provider ended answered %q", results["ended"])
		}
		// A job that was accepted and then lost names itself for a later call.
		if !strings.Contains(results["lost"], `call generate_video again with "job": "video_2"`) {
			t.Fatalf("a job lost after it was accepted answered %q", results["lost"])
		}
		saved, err := os.ReadFile(filepath.Join(root, "assets", "generated", "wave.mp4"))
		if err != nil || !bytes.Equal(saved, movie) || !strings.Contains(results["fetched"], "assets/generated/wave.mp4") || !strings.Contains(results["fetched"], "video_2") {
			t.Fatalf("the named job was not fetched: %d bytes, %v, %q", len(saved), err, results["fetched"])
		}
		if submitted := seen("/v1/videos"); len(submitted) != 2 || submitted[1]["prompt"] != "A wave" {
			t.Fatalf("three calls submitted %d video jobs: %v", len(submitted), submitted)
		}
		// Only the call that saved the video counts its usage.
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs, query.Dimension = time.Now().Add(-time.Hour).UnixMilli(), time.Now().Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
		counted := 0
		for _, item := range commandFixture(t, node, protocol.Command{Kind: "read_usage", Data: query}).Data.(protocol.UsageReport).Requests.Items {
			if item.Session == session.ID && item.Model == "filmer" && item.Tokens.Input == 4 {
				counted++
			}
		}
		if counted != 1 {
			t.Fatalf("the fetched video was counted %d times", counted)
		}
		inv := &store.AgentInvocation{Session: session}
		inv.Worktree.Path = root
		specs, err := operations.MediaSpecs()
		if err != nil {
			t.Fatal(err)
		}
		for _, spec := range specs {
			kind, _ := mediaOperationKind(spec.Operation)
			if kind == protocol.MediaKindVision {
				continue
			}
			generate, err := node.mediaTool(inv, spec, kind, mediaRoute{})
			if err != nil {
				t.Fatal(err)
			}
			jobs := []string{"video_9"}
			if kind == protocol.MediaKindVideo {
				jobs = []string{"", "..", "a/b", "a?b", strings.Repeat("a", 129)}
			}
			for _, job := range jobs {
				arguments, _ := json.Marshal(map[string]string{"prompt": "A wave", "job": job})
				err := generate.Prepare(t.Context(), canonical.ToolRequest{ID: "job", Arguments: arguments})
				var fault *protocol.Fault
				if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("%s accepted the job %q: %v", spec.Name, job, err)
				}
			}
		}
	})

	t.Run("no capable model offers no tool", func(t *testing.T) {
		// Nothing is assigned and the conversation model has no media tag.
		current := commandFixture(t, node, protocol.Command{Kind: "read_media_settings"}).Data.(protocol.MediaSettings)
		cleared := assign(current.Revision, map[protocol.MediaKind]string{})
		before := len(seen("/v1/images/generations"))
		run(t, "bare", protocol.WorkModeCode)
		bare.mu.Lock()
		offered, results := bare.offered, bare.results
		bare.mu.Unlock()
		if len(offered) != 2 {
			t.Fatalf("turn made %d model requests: %v", len(offered), results)
		}
		for _, names := range offered {
			for _, name := range []string{"generate_image", "inspect_image", "generate_video"} {
				if slices.Contains(names, name) {
					t.Fatalf("%s was offered without a capable model: %v", name, names)
				}
			}
		}
		if strings.Contains(results["discover"], "generate_image") || len(seen("/v1/images/generations")) != before {
			t.Fatalf("tool search found a media tool: %q", results["discover"])
		}
		assign(cleared.Revision, map[protocol.MediaKind]string{protocol.MediaKindVision: "eyes", protocol.MediaKindImage: "painter", protocol.MediaKindSpeech: "voice"})
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
