package model

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"image"
	"image/jpeg"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestMediaDrivers checks channel selection and every media request shape
// against a local fake. No provider is contacted.
func TestMediaDrivers(t *testing.T) {
	var pixels bytes.Buffer
	if err := png.Encode(&pixels, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	encoded := base64.StdEncoding.EncodeToString(pixels.Bytes())
	type seen struct {
		Method, Path, Authorization, Key string
		Body                             map[string]any
	}
	var mu sync.Mutex
	var requests []seen
	handlers := map[string]func(http.ResponseWriter, *http.Request){}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		record := seen{Method: request.Method, Path: request.URL.Path, Authorization: request.Header.Get("Authorization"), Key: request.Header.Get("x-goog-api-key")}
		raw, _ := io.ReadAll(request.Body)
		_ = json.Unmarshal(raw, &record.Body)
		mu.Lock()
		requests = append(requests, record)
		handler := handlers[request.Method+" "+request.URL.Path]
		mu.Unlock()
		if handler == nil {
			http.NotFound(writer, request)
			return
		}
		handler(writer, request)
	}))
	defer server.Close()
	handle := func(route string, handler func(http.ResponseWriter, *http.Request)) {
		mu.Lock()
		handlers[route] = handler
		mu.Unlock()
	}
	last := func() seen {
		mu.Lock()
		defer mu.Unlock()
		return requests[len(requests)-1]
	}
	secret := protocol.NewSecret("media-secret")
	bind := func(api protocol.ConversationModelApi, endpoint, name string) Binding {
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Fixture", true
		provider.Api, provider.Authentication, provider.Endpoint = api, protocol.AuthenticationApiKey, server.URL+endpoint
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, name, protocol.WorkModeCode, protocol.PermissionAsk
		binding, err := Capture(provider, config, &secret, protocol.NewSessionID())
		if err != nil {
			t.Fatal(err)
		}
		return binding
	}
	failure := func(err error, kind FailureKind) *Failure {
		t.Helper()
		var failed *Failure
		if !errors.As(err, &failed) || failed.Kind != kind {
			t.Fatalf("failure = %v, want %s", err, kind)
		}
		return failed
	}

	t.Run("channel follows tags, API and authentication", func(t *testing.T) {
		entry := func(vision bool, generates ...protocol.MediaGeneration) protocol.ConversationModel {
			value := protocol.DefaultConversationModel()
			value.ID, value.Vision, value.Generates = "m", vision, generates
			return value
		}
		provider := func(api protocol.ConversationModelApi, authentication protocol.Authentication) protocol.ConversationProvider {
			value := protocol.DefaultConversationProvider()
			value.Api, value.Authentication = api, authentication
			return value
		}
		chat, responses, gemini, anthropic := protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiResponses, protocol.ConversationModelApiGemini, protocol.ConversationModelApiAnthropic
		key, account := protocol.AuthenticationApiKey, protocol.AuthenticationChatGptOAuth
		all := []protocol.MediaGeneration{protocol.MediaGenerationImage, protocol.MediaGenerationImageTool, protocol.MediaGenerationVideo, protocol.MediaGenerationSpeech}
		for _, test := range []struct {
			name     string
			kind     protocol.MediaKind
			provider protocol.ConversationProvider
			model    protocol.ConversationModel
			want     MediaChannel
		}{
			{"vision tag", protocol.MediaKindVision, provider(anthropic, key), entry(true), MediaChat},
			{"no vision tag", protocol.MediaKindVision, provider(anthropic, key), entry(false, all...), ""},
			{"images endpoint", protocol.MediaKindImage, provider(chat, key), entry(false, protocol.MediaGenerationImage), MediaImages},
			{"images endpoint wins over hosted tool", protocol.MediaKindImage, provider(responses, key), entry(false, all...), MediaImages},
			{"hosted tool", protocol.MediaKindImage, provider(responses, key), entry(false, protocol.MediaGenerationImageTool), MediaImageTool},
			{"hosted tool on account", protocol.MediaKindImage, provider(responses, account), entry(false, all...), MediaImageTool},
			{"hosted tool needs Responses", protocol.MediaKindImage, provider(chat, key), entry(false, protocol.MediaGenerationImageTool), ""},
			{"Gemini native", protocol.MediaKindImage, provider(gemini, key), entry(false, protocol.MediaGenerationImage), MediaGemini},
			{"image tag required", protocol.MediaKindImage, provider(chat, key), entry(true, protocol.MediaGenerationVideo, protocol.MediaGenerationSpeech), ""},
			{"image has no Anthropic driver", protocol.MediaKindImage, provider(anthropic, key), entry(false, all...), ""},
			{"video", protocol.MediaKindVideo, provider(responses, key), entry(false, protocol.MediaGenerationVideo), MediaVideos},
			{"video tag required", protocol.MediaKindVideo, provider(chat, key), entry(true, protocol.MediaGenerationImage), ""},
			{"video needs an API key", protocol.MediaKindVideo, provider(responses, account), entry(false, all...), ""},
			{"video has no Gemini driver", protocol.MediaKindVideo, provider(gemini, key), entry(false, all...), ""},
			{"speech", protocol.MediaKindSpeech, provider(chat, key), entry(false, protocol.MediaGenerationSpeech), MediaSpeech},
			{"speech tag required", protocol.MediaKindSpeech, provider(chat, key), entry(true, protocol.MediaGenerationImage), ""},
			{"speech needs an API key", protocol.MediaKindSpeech, provider(responses, account), entry(false, all...), ""},
		} {
			channel, supported := MediaChannelFor(test.kind, test.provider, test.model)
			if channel != test.want || supported != (test.want != "") {
				t.Errorf("%s: channel = %q, %v; want %q", test.name, channel, supported, test.want)
			}
		}
	})

	t.Run("images endpoint", func(t *testing.T) {
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"data":[{"b64_json":"` + encoded + `"}],"usage":{"input_tokens":11,"output_tokens":22,"total_tokens":33}}`))
		})
		output, err := GenerateImage(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "painter"), MediaImages, "a lighthouse")
		if err != nil {
			t.Fatal(err)
		}
		request := last()
		if request.Authorization != "Bearer media-secret" || !reflect.DeepEqual(request.Body, map[string]any{"model": "painter", "prompt": "a lighthouse"}) {
			t.Fatalf("images request = %+v", request)
		}
		if !bytes.Equal(output.Data, pixels.Bytes()) || output.MediaType != "image/png" || *output.Usage.Input != 11 || *output.Usage.Output != 22 || *output.Usage.Total != 33 {
			t.Fatalf("images output = %s %+v", output.MediaType, output.Usage)
		}

		// A hosted result on the provider's origin is downloaded; a JPEG is
		// re-encoded so the saved file is a real PNG.
		var photo bytes.Buffer
		if err := jpeg.Encode(&photo, image.NewRGBA(image.Rect(0, 0, 3, 3)), nil); err != nil {
			t.Fatal(err)
		}
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"data":[{"url":"` + server.URL + `/files/result.jpg"}]}`))
		})
		handle("GET /files/result.jpg", func(writer http.ResponseWriter, _ *http.Request) { _, _ = writer.Write(photo.Bytes()) })
		output, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiResponses, "/v1", "painter"), MediaImages, "a harbor")
		if err != nil {
			t.Fatal(err)
		}
		if _, format, err := image.DecodeConfig(bytes.NewReader(output.Data)); err != nil || format != "png" || *output.Usage.Input != 0 || *output.Usage.Output != 0 {
			t.Fatalf("downloaded image format = %q, %v", format, err)
		}
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"data":[{"url":"http://other.invalid/result.png"}]}`))
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "painter"), MediaImages, "a harbor")
		failure(err, InvalidResponse)
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"data":[{"b64_json":"` + base64.StdEncoding.EncodeToString([]byte("not an image")) + `"}]}`))
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "painter"), MediaImages, "a harbor")
		failure(err, InvalidResponse)
	})

	t.Run("provider refusal is classified, bounded and redacted", func(t *testing.T) {
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			writer.WriteHeader(http.StatusUnauthorized)
			_, _ = writer.Write([]byte(`{"error":{"message":"key media-secret rejected ` + strings.Repeat("x", 600) + `"}}`))
		})
		_, err := GenerateImage(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "painter"), MediaImages, "PROMPT-TEXT")
		failed := failure(err, Authentication)
		if strings.Contains(failed.Message, "media-secret") || strings.Contains(failed.Message, "PROMPT-TEXT") || !strings.Contains(failed.Message, "[redacted]") || len(failed.Message) > 400 {
			t.Fatalf("refusal message = %q", failed.Message)
		}
		// A credential that straddles the length bound leaves no prefix behind.
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			writer.WriteHeader(http.StatusUnauthorized)
			_, _ = writer.Write([]byte(`{"error":{"message":"` + strings.Repeat("x", 291) + `media-secret rejected"}}`))
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "painter"), MediaImages, "a harbor")
		if failed := failure(err, Authentication); strings.Contains(failed.Message, "media-") || !strings.Contains(failed.Message, strings.Repeat("x", 291)+"[redacte") || len(failed.Message) > 400 {
			t.Fatalf("refusal message with a credential at the bound = %q", failed.Message)
		}
		handle("POST /v1/images/generations", func(writer http.ResponseWriter, _ *http.Request) {
			writer.Header().Set("Location", server.URL+"/elsewhere")
			writer.WriteHeader(http.StatusFound)
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "painter"), MediaImages, "a harbor")
		failure(err, InvalidResponse)
	})

	t.Run("hosted image tool", func(t *testing.T) {
		handle("POST /v1/responses", func(writer http.ResponseWriter, _ *http.Request) {
			writer.Header().Set("Content-Type", "text/event-stream")
			for _, event := range []string{
				`{"type":"response.created"}`,
				`{"type":"response.output_item.done","item":{"type":"image_generation_call","result":"` + encoded + `"}}`,
				`{"type":"response.completed","response":{"usage":{"input_tokens":5,"output_tokens":7,"total_tokens":12}}}`,
			} {
				_, _ = writer.Write([]byte("event: message\ndata: " + event + "\n\n"))
			}
		})
		output, err := GenerateImage(t.Context(), bind(protocol.ConversationModelApiResponses, "/v1", "gpt-fixture"), MediaImageTool, "a fox")
		if err != nil {
			t.Fatal(err)
		}
		want := map[string]any{
			"model": "gpt-fixture", "store": false, "stream": true, "max_tool_calls": float64(1),
			"input":       []any{map[string]any{"role": "user", "content": "a fox"}},
			"tools":       []any{map[string]any{"type": "image_generation", "output_format": "png"}},
			"tool_choice": map[string]any{"type": "image_generation"},
		}
		if request := last(); request.Authorization != "Bearer media-secret" || !reflect.DeepEqual(request.Body, want) {
			t.Fatalf("hosted tool request = %+v", request)
		}
		if !bytes.Equal(output.Data, pixels.Bytes()) || *output.Usage.Input != 5 || *output.Usage.Output != 7 {
			t.Fatalf("hosted tool output usage = %+v", output.Usage)
		}
		handle("POST /v1/responses", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`data: {"type":"response.failed","response":{"error":{"message":"moderation blocked"}}}` + "\n\n"))
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiResponses, "/v1", "gpt-fixture"), MediaImageTool, "a fox")
		if failed := failure(err, Unavailable); !strings.Contains(failed.Message, "moderation blocked") {
			t.Fatalf("hosted tool failure = %q", failed.Message)
		}
		handle("POST /v1/responses", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`data: {"type":"response.output_item.done","item":{"type":"image_generation_call","result":"` + encoded + `"}}` + "\n\n"))
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiResponses, "/v1", "gpt-fixture"), MediaImageTool, "a fox")
		failure(err, InvalidResponse)
	})

	t.Run("Gemini native image", func(t *testing.T) {
		handle("POST /v1beta/models/gemini-image:generateContent", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"candidates":[{"content":{"parts":[{"text":"Here it is"},{"inlineData":{"mimeType":"image/png","data":"` + encoded + `"}}]}}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":1290,"totalTokenCount":1293}}`))
		})
		output, err := GenerateImage(t.Context(), bind(protocol.ConversationModelApiGemini, "/v1beta", "gemini-image"), MediaGemini, "a kite")
		if err != nil {
			t.Fatal(err)
		}
		want := map[string]any{
			"contents":         []any{map[string]any{"role": "user", "parts": []any{map[string]any{"text": "a kite"}}}},
			"generationConfig": map[string]any{"responseModalities": []any{"TEXT", "IMAGE"}},
		}
		if request := last(); request.Key != "media-secret" || request.Authorization != "" || !reflect.DeepEqual(request.Body, want) {
			t.Fatalf("Gemini request = %+v", request)
		}
		if !bytes.Equal(output.Data, pixels.Bytes()) || *output.Usage.Input != 3 || *output.Usage.Output != 1290 || *output.Usage.Total != 1293 {
			t.Fatalf("Gemini output usage = %+v", output.Usage)
		}
		handle("POST /v1beta/models/gemini-image:generateContent", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"candidates":[{"content":{"parts":[{"text":"I cannot draw that"}]}}]}`))
		})
		_, err = GenerateImage(t.Context(), bind(protocol.ConversationModelApiGemini, "/v1beta", "gemini-image"), MediaGemini, "a kite")
		failure(err, InvalidResponse)
	})

	t.Run("video job", func(t *testing.T) {
		movie := append([]byte{0, 0, 0, 24}, []byte("ftypmp42 fixture video")...)
		var polls atomic.Int32
		handle("POST /v1/videos", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"id":"video_1","status":"queued"}`))
		})
		handle("GET /v1/videos/video_1", func(writer http.ResponseWriter, _ *http.Request) {
			if polls.Add(1) < 2 {
				_, _ = writer.Write([]byte(`{"id":"video_1","status":"in_progress"}`))
				return
			}
			_, _ = writer.Write([]byte(`{"id":"video_1","status":"completed","usage":{"input_tokens":4,"output_tokens":0}}`))
		})
		handle("GET /v1/videos/video_1/content", func(writer http.ResponseWriter, _ *http.Request) { _, _ = writer.Write(movie) })
		binding := bind(protocol.ConversationModelApiChatCompletions, "/v1", "director")
		output, err := GenerateVideo(t.Context(), binding, "a wave", time.Millisecond)
		if err != nil {
			t.Fatal(err)
		}
		mu.Lock()
		submitted := requests[len(requests)-4]
		mu.Unlock()
		if polls.Load() != 2 || submitted.Path != "/v1/videos" || !reflect.DeepEqual(submitted.Body, map[string]any{"model": "director", "prompt": "a wave"}) {
			t.Fatalf("video submission = %+v after %d polls", submitted, polls.Load())
		}
		if !bytes.Equal(output.Data, movie) || output.MediaType != "video/mp4" || output.Job != "video_1" || *output.Usage.Input != 4 {
			t.Fatalf("video output = %s %s", output.MediaType, output.Job)
		}
		handle("GET /v1/videos/video_1", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"id":"video_1","status":"failed","error":{"message":"content policy"}}`))
		})
		output, err = GenerateVideo(t.Context(), binding, "a wave", time.Millisecond)
		if failed := failure(err, Unavailable); output.Job != "video_1" || !strings.Contains(failed.Message, "content policy") {
			t.Fatalf("failed video = %q, job %q", failed.Message, output.Job)
		}
		handle("GET /v1/videos/video_1", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"id":"video_1","status":"completed"}`))
		})
		handle("GET /v1/videos/video_1/content", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte("<html>not a video</html>"))
		})
		_, err = GenerateVideo(t.Context(), binding, "a wave", time.Millisecond)
		failure(err, InvalidResponse)
		// A local stop reports cancellation and keeps the accepted job.
		handle("GET /v1/videos/video_1", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write([]byte(`{"id":"video_1","status":"in_progress"}`))
		})
		stopped, stop := context.WithCancel(t.Context())
		time.AfterFunc(20*time.Millisecond, stop)
		output, err = GenerateVideo(stopped, binding, "a wave", time.Millisecond)
		if failure(err, Cancelled); output.Job != "video_1" {
			t.Fatalf("stopped video job = %q", output.Job)
		}
	})

	t.Run("speech endpoint", func(t *testing.T) {
		handle("POST /v1/audio/speech", func(writer http.ResponseWriter, _ *http.Request) {
			writer.Header().Set("Content-Type", "audio/mpeg")
			_, _ = writer.Write([]byte("ID3 fixture audio"))
		})
		output, err := SynthesizeSpeech(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "voice"), "Read this aloud", "alloy")
		if err != nil {
			t.Fatal(err)
		}
		want := map[string]any{"model": "voice", "input": "Read this aloud", "voice": "alloy", "response_format": "mp3"}
		if request := last(); request.Authorization != "Bearer media-secret" || !reflect.DeepEqual(request.Body, want) {
			t.Fatalf("speech request = %+v", request)
		}
		if string(output.Data) != "ID3 fixture audio" || output.MediaType != "audio/mpeg" {
			t.Fatalf("speech output = %q %s", output.Data, output.MediaType)
		}
		handle("POST /v1/audio/speech", func(writer http.ResponseWriter, _ *http.Request) {
			_, _ = writer.Write(make([]byte, MaxMediaSpeechBytes+1))
		})
		_, err = SynthesizeSpeech(t.Context(), bind(protocol.ConversationModelApiChatCompletions, "/v1", "voice"), "Read this aloud", "alloy")
		failure(err, InvalidResponse)
	})
}
