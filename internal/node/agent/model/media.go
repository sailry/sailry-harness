package model

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	_ "image/jpeg" // provider images that are re-encoded as PNG
	"image/png"
	"io"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	_ "golang.org/x/image/webp" // provider images that are re-encoded as PNG
)

// MediaChannel names the provider endpoint that serves one media kind.
type MediaChannel string

const (
	// MediaChat sends an image part through the model's normal chat request.
	MediaChat MediaChannel = "chat"
	// MediaImages is the OpenAI-compatible POST /images/generations endpoint.
	MediaImages MediaChannel = "images"
	// MediaImageTool is the Responses API hosted image generation tool.
	MediaImageTool MediaChannel = "image_tool"
	// MediaGemini is Gemini's native generateContent image output.
	MediaGemini MediaChannel = "gemini"
	// MediaVideos is the OpenAI-compatible POST /videos job endpoint.
	MediaVideos MediaChannel = "videos"
	// MediaSpeech is the OpenAI-compatible POST /audio/speech endpoint.
	MediaSpeech MediaChannel = "speech"
)

const (
	MaxMediaImageBytes  = 64 << 20
	MaxMediaVideoBytes  = 512 << 20
	MaxMediaSpeechBytes = 8 << 20
	// MaxSpeechCharacters is the input bound of the speech endpoint.
	MaxSpeechCharacters = 4096
	// MediaVideoPoll is the production interval between video job reads.
	MediaVideoPoll = 5 * time.Second

	mediaImageDeadline  = 240 * time.Second
	mediaVideoDeadline  = 540 * time.Second
	mediaSpeechDeadline = 120 * time.Second
	mediaJSONBytes      = 1 << 20
)

// MediaChannelFor selects the channel driver for one media kind from the
// model's capability tags and its provider's API and authentication. A model
// without the tag, or on a channel without a driver, has no route.
func MediaChannelFor(kind protocol.MediaKind, provider protocol.ConversationProvider, entry protocol.ConversationModel) (MediaChannel, bool) {
	keyed := provider.Authentication == protocol.AuthenticationApiKey
	compatible := provider.Api == protocol.ConversationModelApiChatCompletions || provider.Api == protocol.ConversationModelApiResponses
	switch kind {
	case protocol.MediaKindVision:
		if !entry.Vision {
			return "", false
		}
		if provider.Api == protocol.ConversationModelApiPlugin {
			return MediaChat, PluginBound(provider) && (**provider.Binding).Adapter != nil && (**provider.Binding).Adapter.Capabilities.Vision
		}
		switch provider.Authentication {
		case protocol.AuthenticationApiKey, protocol.AuthenticationPlugin:
			return MediaChat, true
		case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
			return MediaChat, provider.Api == protocol.ConversationModelApiResponses
		case protocol.AuthenticationCopilot:
			return MediaChat, compatible
		case protocol.AuthenticationClaudeOAuth:
			return MediaChat, provider.Api == protocol.ConversationModelApiAnthropic
		}
	case protocol.MediaKindImage:
		if slices.Contains(entry.Generates, protocol.MediaGenerationImage) && keyed {
			if compatible {
				return MediaImages, true
			}
			if provider.Api == protocol.ConversationModelApiGemini {
				return MediaGemini, true
			}
		}
		if slices.Contains(entry.Generates, protocol.MediaGenerationImageTool) && provider.Api == protocol.ConversationModelApiResponses &&
			(keyed || provider.Authentication == protocol.AuthenticationChatGptOAuth || provider.Authentication == protocol.AuthenticationCopilot) {
			return MediaImageTool, true
		}
	case protocol.MediaKindVideo:
		if slices.Contains(entry.Generates, protocol.MediaGenerationVideo) && keyed && compatible {
			return MediaVideos, true
		}
	case protocol.MediaKindSpeech:
		if slices.Contains(entry.Generates, protocol.MediaGenerationSpeech) && keyed && compatible {
			return MediaSpeech, true
		}
	}
	return "", false
}

// MediaOutput is one generated asset and the usage its provider reported.
type MediaOutput struct {
	Data      []byte
	MediaType string
	Usage     canonical.Usage
	// Job identifies an accepted provider job that outlives a local stop.
	Job string
}

type mediaCall struct {
	binding Binding
	config  transportConfig
	client  *http.Client
	secrets []protocol.Secret
}

func newMediaCall(binding Binding) (*mediaCall, error) {
	config, err := transportBinding(binding)
	if err != nil {
		return nil, err
	}
	call := &mediaCall{binding: binding, config: config, client: &http.Client{Transport: netproxy.Transport(), CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
	if binding.Secret != nil {
		call.secrets = []protocol.Secret{*binding.Secret}
	}
	call.secrets = append(call.secrets, config.Protection...)
	return call, nil
}

// request authorizes one provider request exactly as the model transport does.
func (call *mediaCall) request(ctx context.Context, method, path string, body any) (*http.Request, error) {
	target, err := url.Parse(call.config.Endpoint)
	if err != nil {
		return nil, err
	}
	target.Path = strings.TrimRight(target.Path, "/") + path
	query := target.Query()
	for key, value := range call.config.Query {
		query.Set(key, value)
	}
	target.RawQuery = query.Encode()
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		reader = bytes.NewReader(encoded)
	}
	request, err := http.NewRequestWithContext(ctx, method, target.String(), reader)
	if err != nil {
		return nil, err
	}
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	for key, value := range call.config.Headers {
		request.Header.Set(key, value)
	}
	return request, nil
}

// send returns a successful response. A provider refusal becomes a bounded,
// redacted failure that never echoes the request.
func (call *mediaCall) send(request *http.Request) (*http.Response, error) {
	response, err := call.client.Do(request)
	if err != nil {
		if request.Context().Err() != nil {
			return nil, mediaContextFailure(request.Context())
		}
		return nil, &Failure{Kind: Network, Message: "media provider request failed"}
	}
	if response.StatusCode >= 200 && response.StatusCode < 300 {
		return response, nil
	}
	defer response.Body.Close()
	body, _ := io.ReadAll(io.LimitReader(response.Body, 64<<10))
	kind := InvalidRequest
	switch {
	case response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden:
		kind = Authentication
	case response.StatusCode == http.StatusTooManyRequests:
		kind = RateLimited
	case response.StatusCode >= 500:
		kind = Unavailable
	case response.StatusCode >= 300 && response.StatusCode < 400:
		kind = InvalidResponse
	}
	status := uint16(response.StatusCode)
	return nil, &Failure{Kind: kind, Message: fmt.Sprintf("media provider returned HTTP %d%s", response.StatusCode, call.diagnostic(body)), status: &status}
}

func (call *mediaCall) diagnostic(body []byte) string {
	var decoded struct {
		Error json.RawMessage `json:"error"`
	}
	if json.Unmarshal(body, &decoded) != nil || len(decoded.Error) == 0 {
		return ""
	}
	var message string
	var object struct {
		Message string `json:"message"`
	}
	if json.Unmarshal(decoded.Error, &message) != nil && json.Unmarshal(decoded.Error, &object) == nil {
		message = object.Message
	}
	return call.detail(message)
}

func (call *mediaCall) detail(message string) string {
	message = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, strings.TrimSpace(message))
	if message == "" {
		return ""
	}
	// Redact before truncation so a cut can never leave a partial credential.
	message = safeDiagnostic(message, call.secrets)
	if len(message) > 300 {
		end := 300
		for end > 0 && !utf8.RuneStart(message[end]) {
			end--
		}
		message = message[:end]
	}
	return ": " + message
}

func mediaContextFailure(ctx context.Context) error {
	if errors.Is(ctx.Err(), context.DeadlineExceeded) {
		return &Failure{Kind: Unavailable, Message: "media provider request timed out"}
	}
	return &Failure{Kind: Cancelled, Message: "media request was cancelled"}
}

// bounded reads a whole body and refuses one byte more than the limit.
func bounded(ctx context.Context, body io.Reader, limit int64) ([]byte, error) {
	data, err := io.ReadAll(io.LimitReader(body, limit+1))
	if err != nil {
		if ctx.Err() != nil {
			return nil, mediaContextFailure(ctx)
		}
		return nil, &Failure{Kind: Network, Message: "media provider response was interrupted"}
	}
	if int64(len(data)) > limit {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider response exceeds the size limit"}
	}
	return data, nil
}

func (call *mediaCall) json(request *http.Request, limit int64, target any) error {
	response, err := call.send(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	data, err := bounded(request.Context(), response.Body, limit)
	if err != nil {
		return err
	}
	if json.Unmarshal(data, target) != nil {
		return &Failure{Kind: InvalidResponse, Message: "media provider returned an invalid response"}
	}
	return nil
}

type mediaTokens struct {
	Input  *uint64 `json:"input_tokens"`
	Output *uint64 `json:"output_tokens"`
	Total  *uint64 `json:"total_tokens"`
}

// usage always reports input and output so the usage tables count the call.
func (tokens *mediaTokens) usage() canonical.Usage {
	var zero uint64
	usage := canonical.Usage{Input: &zero, Output: &zero}
	if tokens == nil {
		return usage
	}
	if tokens.Input != nil {
		usage.Input = tokens.Input
	}
	if tokens.Output != nil {
		usage.Output = tokens.Output
	}
	usage.Total = tokens.Total
	return usage
}

// base64 output is bounded by its encoded length before decoding.
func decodeMedia(encoded string, limit int) ([]byte, error) {
	if base64.StdEncoding.DecodedLen(len(encoded)) > limit+3 {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider response exceeds the size limit"}
	}
	data, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil || len(data) == 0 {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider returned invalid image data"}
	}
	if len(data) > limit {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider response exceeds the size limit"}
	}
	return data, nil
}

// pngImage keeps provider PNG bytes and re-encodes any other decodable image,
// because the generation tools promise a real PNG file.
func pngImage(data []byte) ([]byte, error) {
	config, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || config.Width <= 0 || config.Height <= 0 || int64(config.Width)*int64(config.Height) > 64<<20 {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider returned an unsupported image"}
	}
	if format == "png" {
		return data, nil
	}
	decoded, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider returned an unsupported image"}
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, decoded); err != nil || encoded.Len() > MaxMediaImageBytes {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider returned an unsupported image"}
	}
	return encoded.Bytes(), nil
}

// GenerateImage runs one image generation on the channel selected for the
// bound model and returns PNG bytes.
func GenerateImage(ctx context.Context, binding Binding, channel MediaChannel, prompt string) (MediaOutput, error) {
	call, err := newMediaCall(binding)
	if err != nil {
		return MediaOutput{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, mediaImageDeadline)
	defer cancel()
	var output MediaOutput
	switch channel {
	case MediaImages:
		output, err = call.images(ctx, prompt)
	case MediaImageTool:
		output, err = call.imageTool(ctx, prompt)
	case MediaGemini:
		output, err = call.gemini(ctx, prompt)
	default:
		return MediaOutput{}, &Failure{Kind: Unsupported, Message: "provider does not support image generation"}
	}
	if err != nil {
		return MediaOutput{}, err
	}
	if output.Data, err = pngImage(output.Data); err != nil {
		return MediaOutput{}, err
	}
	output.MediaType = "image/png"
	return output, nil
}

func (call *mediaCall) images(ctx context.Context, prompt string) (MediaOutput, error) {
	request, err := call.request(ctx, http.MethodPost, "/images/generations", map[string]any{"model": call.config.Model, "prompt": prompt})
	if err != nil {
		return MediaOutput{}, err
	}
	var decoded struct {
		Data []struct {
			Encoded string `json:"b64_json"`
			URL     string `json:"url"`
		} `json:"data"`
		Usage *mediaTokens `json:"usage"`
	}
	if err := call.json(request, MaxMediaImageBytes*4/3+mediaJSONBytes, &decoded); err != nil {
		return MediaOutput{}, err
	}
	if len(decoded.Data) == 0 {
		return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider returned no image"}
	}
	output := MediaOutput{Usage: decoded.Usage.usage()}
	if decoded.Data[0].Encoded != "" {
		output.Data, err = decodeMedia(decoded.Data[0].Encoded, MaxMediaImageBytes)
		return output, err
	}
	output.Data, err = call.download(ctx, decoded.Data[0].URL, MaxMediaImageBytes)
	return output, err
}

// download reads a provider-hosted result without the provider's credentials
// unless the result lives on the provider's own origin.
func (call *mediaCall) download(ctx context.Context, location string, limit int64) ([]byte, error) {
	target, err := url.Parse(location)
	base, baseErr := url.Parse(call.config.Endpoint)
	if err != nil || baseErr != nil || target.Host == "" || target.User != nil {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider returned no image"}
	}
	same := target.Scheme == base.Scheme && target.Host == base.Host
	if target.Scheme != "https" && !same {
		return nil, &Failure{Kind: InvalidResponse, Message: "media provider returned an insecure result location"}
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, target.String(), nil)
	if err != nil {
		return nil, err
	}
	if same {
		for key, value := range call.config.Headers {
			request.Header.Set(key, value)
		}
	}
	response, err := call.send(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	return bounded(ctx, response.Body, limit)
}

// imageTool asks the Responses API to run its hosted image generation tool and
// reads the streamed result.
func (call *mediaCall) imageTool(ctx context.Context, prompt string) (MediaOutput, error) {
	body := map[string]any{}
	for key, value := range call.config.JSON {
		body[key] = value
	}
	body["model"], body["store"], body["stream"] = call.config.Model, false, true
	body["tools"] = []any{map[string]any{"type": "image_generation", "output_format": "png"}}
	body["tool_choice"] = map[string]any{"type": "image_generation"}
	if call.binding.Provider.Authentication == protocol.AuthenticationApiKey {
		body["input"] = []any{map[string]any{"role": "user", "content": prompt}}
		body["max_tool_calls"] = 1
	} else {
		body["input"] = []any{map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": prompt}}}}
	}
	request, err := call.request(ctx, http.MethodPost, "/responses", body)
	if err != nil {
		return MediaOutput{}, err
	}
	request.Header.Set("Accept", "text/event-stream")
	response, err := call.send(request)
	if err != nil {
		return MediaOutput{}, err
	}
	defer response.Body.Close()
	const line = MaxMediaImageBytes*4/3 + mediaJSONBytes
	reader := bufio.NewReaderSize(io.LimitReader(response.Body, 2*line), 64<<10)
	var output MediaOutput
	var encoded string
	terminal := false
	for !terminal {
		data, err := reader.ReadSlice('\n')
		var long []byte
		for errors.Is(err, bufio.ErrBufferFull) {
			if len(long)+len(data) > line {
				return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider response exceeds the size limit"}
			}
			long = append(long, data...)
			data, err = reader.ReadSlice('\n')
		}
		if long != nil {
			data = append(long, data...)
		}
		if payload, ok := bytes.CutPrefix(bytes.TrimSpace(data), []byte("data:")); ok {
			var event struct {
				Type string `json:"type"`
				Item struct {
					Type   string `json:"type"`
					Result string `json:"result"`
				} `json:"item"`
				Response struct {
					Usage *mediaTokens `json:"usage"`
					Error *struct {
						Message string `json:"message"`
					} `json:"error"`
				} `json:"response"`
				Message string `json:"message"`
			}
			if json.Unmarshal(bytes.TrimSpace(payload), &event) == nil {
				switch event.Type {
				case "response.output_item.done":
					if event.Item.Type == "image_generation_call" && event.Item.Result != "" {
						encoded = event.Item.Result
					}
				case "response.completed":
					output.Usage, terminal = event.Response.Usage.usage(), true
				case "response.incomplete", "response.failed", "error":
					detail := event.Message
					if event.Response.Error != nil {
						detail = event.Response.Error.Message
					}
					return MediaOutput{}, &Failure{Kind: Unavailable, Message: "image generation did not complete" + call.detail(detail)}
				}
			}
		}
		if err != nil {
			if ctx.Err() != nil {
				return MediaOutput{}, mediaContextFailure(ctx)
			}
			break
		}
	}
	if !terminal {
		return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider response ended before completion"}
	}
	if encoded == "" {
		return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider returned no image"}
	}
	output.Data, err = decodeMedia(encoded, MaxMediaImageBytes)
	return output, err
}

func (call *mediaCall) gemini(ctx context.Context, prompt string) (MediaOutput, error) {
	name := strings.TrimPrefix(call.config.Model, "models/")
	if name == "" || strings.ContainsAny(name, "/:") || strings.ContainsFunc(name, unicode.IsControl) {
		return MediaOutput{}, &Failure{Kind: InvalidRequest, Message: "Gemini image model identifier is invalid"}
	}
	version := "/v1beta"
	if strings.HasSuffix(strings.TrimRight(call.config.Endpoint, "/"), "/v1") {
		version = ""
	}
	request, err := call.request(ctx, http.MethodPost, version+"/models/"+url.PathEscape(name)+":generateContent", map[string]any{
		"contents":         []any{map[string]any{"role": "user", "parts": []any{map[string]any{"text": prompt}}}},
		"generationConfig": map[string]any{"responseModalities": []string{"TEXT", "IMAGE"}},
	})
	if err != nil {
		return MediaOutput{}, err
	}
	var decoded struct {
		Candidates []struct {
			Content struct {
				Parts []struct {
					Inline *struct {
						Data string `json:"data"`
					} `json:"inlineData"`
				} `json:"parts"`
			} `json:"content"`
		} `json:"candidates"`
		Usage *struct {
			Prompt     *uint64 `json:"promptTokenCount"`
			Candidates *uint64 `json:"candidatesTokenCount"`
			Total      *uint64 `json:"totalTokenCount"`
		} `json:"usageMetadata"`
	}
	if err := call.json(request, MaxMediaImageBytes*4/3+mediaJSONBytes, &decoded); err != nil {
		return MediaOutput{}, err
	}
	var tokens *mediaTokens
	if decoded.Usage != nil {
		tokens = &mediaTokens{Input: decoded.Usage.Prompt, Output: decoded.Usage.Candidates, Total: decoded.Usage.Total}
	}
	output := MediaOutput{Usage: tokens.usage()}
	for _, candidate := range decoded.Candidates {
		for _, part := range candidate.Content.Parts {
			if part.Inline != nil && part.Inline.Data != "" {
				output.Data, err = decodeMedia(part.Inline.Data, MaxMediaImageBytes)
				return output, err
			}
		}
	}
	return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider returned no image"}
}

// GenerateVideo submits one OpenAI-compatible video job, waits for it and
// downloads the MP4. Stopping locally does not cancel an accepted job.
func GenerateVideo(ctx context.Context, binding Binding, prompt string, poll time.Duration) (MediaOutput, error) {
	call, err := newMediaCall(binding)
	if err != nil {
		return MediaOutput{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, mediaVideoDeadline)
	defer cancel()
	type job struct {
		ID     string       `json:"id"`
		Status string       `json:"status"`
		Usage  *mediaTokens `json:"usage"`
		Error  *struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	request, err := call.request(ctx, http.MethodPost, "/videos", map[string]any{"model": call.config.Model, "prompt": prompt})
	if err != nil {
		return MediaOutput{}, err
	}
	var current job
	if err := call.json(request, mediaJSONBytes, &current); err != nil {
		return MediaOutput{}, err
	}
	id := current.ID
	if !safeSegment(id) {
		return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider returned an invalid video job"}
	}
	output := MediaOutput{Job: id}
	timer := time.NewTimer(poll)
	defer timer.Stop()
	for {
		switch current.Status {
		case "completed":
			output.Usage = current.Usage.usage()
			request, err := call.request(ctx, http.MethodGet, "/videos/"+id+"/content", nil)
			if err != nil {
				return output, err
			}
			response, err := call.send(request)
			if err != nil {
				return output, err
			}
			defer response.Body.Close()
			if output.Data, err = bounded(ctx, response.Body, MaxMediaVideoBytes); err != nil {
				return output, err
			}
			if len(output.Data) < 12 || string(output.Data[4:8]) != "ftyp" {
				return output, &Failure{Kind: InvalidResponse, Message: "media provider returned a video that is not MP4"}
			}
			output.MediaType = "video/mp4"
			return output, nil
		case "queued", "in_progress":
		case "failed", "cancelled":
			detail := ""
			if current.Error != nil {
				detail = call.detail(current.Error.Message)
			}
			return output, &Failure{Kind: Unavailable, Message: "video generation " + current.Status + detail}
		default:
			return output, &Failure{Kind: InvalidResponse, Message: "media provider returned an unknown video status"}
		}
		select {
		case <-ctx.Done():
			return output, mediaContextFailure(ctx)
		case <-timer.C:
			timer.Reset(poll)
		}
		request, err := call.request(ctx, http.MethodGet, "/videos/"+id, nil)
		if err != nil {
			return output, err
		}
		current = job{}
		if err := call.json(request, mediaJSONBytes, &current); err != nil {
			return output, err
		}
	}
}

// SynthesizeSpeech reads text aloud through the OpenAI-compatible speech
// endpoint and returns MP3 audio.
func SynthesizeSpeech(ctx context.Context, binding Binding, text, voice string) (MediaOutput, error) {
	call, err := newMediaCall(binding)
	if err != nil {
		return MediaOutput{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, mediaSpeechDeadline)
	defer cancel()
	request, err := call.request(ctx, http.MethodPost, "/audio/speech", map[string]any{"model": call.config.Model, "input": text, "voice": voice, "response_format": "mp3"})
	if err != nil {
		return MediaOutput{}, err
	}
	response, err := call.send(request)
	if err != nil {
		return MediaOutput{}, err
	}
	defer response.Body.Close()
	data, err := bounded(ctx, response.Body, MaxMediaSpeechBytes)
	if err != nil {
		return MediaOutput{}, err
	}
	if len(data) == 0 {
		return MediaOutput{}, &Failure{Kind: InvalidResponse, Message: "media provider returned no audio"}
	}
	return MediaOutput{Data: data, MediaType: "audio/mpeg", Usage: (*mediaTokens)(nil).usage()}, nil
}
