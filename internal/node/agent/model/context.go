// Context discovery policy derived from Goose 14741e9a providers/openai.rs
// (Apache-2.0). See LICENSE.goose.
package model

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// defaultContext is the window assumed for a model nothing describes.
const defaultContext = 128000

const (
	contextProbeTimeout = 5 * time.Second
	contextKnown        = 15 * time.Minute
	contextFailed       = 60 * time.Second
	contextCapacity     = 128
)

type contextEntry struct {
	limit   uint32
	expires time.Time
}

var contextCache = struct {
	sync.Mutex
	entries map[string]contextEntry
}{entries: map[string]contextEntry{}}

// ContextLimit returns the context window of the bound model: the size its
// provider lists, else the size its server reported when the model was created,
// else the bundled catalog's size, else 128000. It performs no I/O.
func ContextLimit(binding Binding) uint32 {
	if listed := metadata(binding); listed != nil && listed.Context > 0 {
		return listed.Context
	}
	if limit, _ := reportedContext(contextKey(binding)); limit > 0 {
		return limit
	}
	return catalogContext(binding.Provider.Api, binding.Config.Model)
}

func contextKey(binding Binding) string {
	return string(binding.Provider.Api) + "\x00" + binding.Provider.Endpoint + "\x00" + binding.Config.Model
}

func reportedContext(key string) (uint32, bool) {
	contextCache.Lock()
	defer contextCache.Unlock()
	entry, ok := contextCache.entries[key]
	if ok && !time.Now().Before(entry.expires) {
		delete(contextCache.entries, key)
		return 0, false
	}
	return entry.limit, ok
}

func rememberContext(key string, limit uint32, life time.Duration) {
	contextCache.Lock()
	defer contextCache.Unlock()
	now := time.Now()
	if len(contextCache.entries) >= contextCapacity {
		oldest := ""
		for name, entry := range contextCache.entries {
			if !now.Before(entry.expires) {
				delete(contextCache.entries, name)
			} else if oldest == "" || entry.expires.Before(contextCache.entries[oldest].expires) {
				oldest = name
			}
		}
		if len(contextCache.entries) >= contextCapacity {
			delete(contextCache.entries, oldest)
		}
	}
	contextCache.entries[key] = contextEntry{limit: limit, expires: now.Add(life)}
}

// discoverContext asks an OpenAI-compatible server for the window it allocated
// to an unlisted model. The answer, or its absence, is kept so that a turn asks
// at most once per period; a cancelled turn keeps nothing.
func discoverContext(ctx context.Context, binding Binding, config transportConfig) {
	if metadata(binding) != nil {
		return
	}
	key := contextKey(binding)
	switch binding.Provider.Api {
	case protocol.ConversationModelApiResponses, protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiDeepSeek, protocol.ConversationModelApiOpenCodeGo, protocol.ConversationModelApiOpenCodeZen:
	case protocol.ConversationModelApiAzureAi:
		// A Foundry deployment is sized by the model it serves.
		if config.Model != binding.Config.Model {
			rememberContext(key, catalogContext(binding.Provider.Api, config.Model), contextKnown)
		}
		return
	default:
		return
	}
	if _, ok := reportedContext(key); ok {
		return
	}
	probe, cancel := context.WithTimeout(ctx, contextProbeTimeout)
	limit, err := allocatedContext(probe, config, binding.Config.Model)
	cancel()
	if ctx.Err() != nil {
		return
	}
	life := contextKnown
	if err != nil {
		life = contextFailed
	}
	rememberContext(key, limit, life)
}

// allocatedContext reads data[].meta.n_ctx from the server's model list: the
// entry of this model, or the only entry of a server that loads one model.
func allocatedContext(ctx context.Context, config transportConfig, name string) (uint32, error) {
	target, err := url.Parse(config.Endpoint + "/models")
	if err != nil {
		return 0, err
	}
	query := target.Query()
	for key, value := range config.Query {
		query.Set(key, value)
	}
	target.RawQuery = query.Encode()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, target.String(), nil)
	if err != nil {
		return 0, err
	}
	for key, value := range config.Headers {
		request.Header.Set(key, value)
	}
	client := &http.Client{Transport: netproxy.Transport(), CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := client.Do(request)
	if err != nil {
		return 0, &Failure{Kind: Network, Message: "model context discovery failed"}
	}
	defer response.Body.Close()
	if response.StatusCode == http.StatusNotFound {
		return 0, nil
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return 0, &Failure{Kind: Network, Message: "model context discovery failed"}
	}
	var body struct {
		Data []struct {
			ID   string `json:"id"`
			Meta struct {
				Context uint32 `json:"n_ctx"`
			} `json:"meta"`
		} `json:"data"`
	}
	if err = json.NewDecoder(io.LimitReader(response.Body, 4<<20)).Decode(&body); err != nil {
		return 0, &Failure{Kind: Network, Message: "model context response is invalid"}
	}
	for _, entry := range body.Data {
		if entry.ID == name {
			return entry.Meta.Context, nil
		}
	}
	if len(body.Data) == 1 {
		return body.Data[0].Meta.Context, nil
	}
	return 0, nil
}

// catalogContext looks a model up in the bundled catalog: under its API's own
// vendor first, then under any vendor, where the smallest window wins so that
// a wrong guess compacts early instead of overflowing.
func catalogContext(api protocol.ConversationModelApi, name string) uint32 {
	family := ""
	switch api {
	case protocol.ConversationModelApiResponses, protocol.ConversationModelApiChatCompletions:
		family = "openai"
	case protocol.ConversationModelApiAnthropic:
		family = "anthropic"
	case protocol.ConversationModelApiGemini:
		family = "gemini-grpc"
	case protocol.ConversationModelApiAzureOpenAi, protocol.ConversationModelApiAzureAi:
		family = "azure.openai"
	case protocol.ConversationModelApiDeepSeek:
		family = "deepseek"
	case protocol.ConversationModelApiOpenCodeGo:
		family = "opencode-go"
	case protocol.ConversationModelApiOpenCodeZen:
		family = "opencode"
	}
	leaf := name[strings.LastIndex(name, "/")+1:]
	for _, candidate := range []string{name, leaf} {
		if entry, ok := catalogEntry(family, candidate); ok && entry.Limit.Context > 0 {
			return entry.Limit.Context
		}
	}
	if limit := catalogWindows()[strings.ToLower(leaf)]; limit > 0 {
		return limit
	}
	return defaultContext
}

var catalogWindows = sync.OnceValue(func() map[string]uint32 {
	catalogEntry("", "")
	windows := map[string]uint32{}
	for _, vendor := range catalog {
		for name, entry := range vendor.Models {
			name = strings.ToLower(name[strings.LastIndex(name, "/")+1:])
			if limit := entry.Limit.Context; limit > 0 && (windows[name] == 0 || limit < windows[name]) {
				windows[name] = limit
			}
		}
	}
	return windows
})
