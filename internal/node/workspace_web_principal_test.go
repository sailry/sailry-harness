package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceSearchKeepsPersonalCredentialsAndCaches(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	var mu sync.Mutex
	calls := map[string]int{}
	users := map[protocol.NodeID]protocol.User{}
	keys := map[string]protocol.UserID{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment, Search: web.SearchOptions{
		Endpoints: map[protocol.SearchDriver]string{protocol.SearchDriverBrave: "https://search.fixture/"},
		Transport: workspaceHTTP(func(request *http.Request) (*http.Response, error) {
			key := request.Header.Get("X-Subscription-Token")
			mu.Lock()
			calls[key]++
			expected, known := keys[key]
			mu.Unlock()
			actor, authenticated := store.UserFromContext(request.Context())
			storage, captured := store.CapturedUserFromContext(request.Context())
			if !known || !authenticated || actor.ID != expected || !captured || storage != expected {
				t.Errorf("search lost its admitted personal context: actor=%+v, storage=%s", actor, storage)
			}
			body, _ := json.Marshal(map[string]any{"web": map[string]any{"results": []any{map[string]string{
				"url": "https://example.com/" + key, "title": key, "description": "Personal fixture result",
			}}}})
			return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(strings.NewReader(string(body)))}, nil
		}),
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	callers := []protocol.NodeID{node.ID(), {95}, {96}}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		for index, caller := range callers {
			if index != 0 {
				if err := store.PairLinkPeer(ctx, db, caller, link.DirectionController); err != nil {
					return err
				}
			}
			user, err := store.CallerUser(ctx, db, node.ID(), caller)
			if err != nil {
				return err
			}
			users[caller] = user
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	call := func(caller protocol.NodeID, command protocol.Command) protocol.Output {
		t.Helper()
		owner := store.WithCapturedUser(ctx, users[caller].ID)
		accepted, err := node.Dispatch(owner, caller, protocol.NewRequest(node.ID(), command))
		if err != nil {
			t.Fatal(err)
		}
		result := <-accepted.Completion
		if result.Fault != nil {
			t.Fatalf("%s: %v", command.Kind, result.Fault)
		}
		return result.Output
	}
	save := func(caller protocol.NodeID, key string) {
		t.Helper()
		output := call(caller, protocol.Command{Kind: "read_search_settings"})
		settings, valid := dispatchPayload[protocol.SearchSettings](output.Data)
		if !valid {
			t.Fatalf("search settings output: %+v", output)
		}
		settings.Driver = &protocol.SearchSelection{Kind: "driver", Data: protocol.SearchDriverBrave}
		settings.Fallback = false
		provider := settings.Providers[protocol.SearchDriverBrave]
		provider.Enabled = true
		settings.Providers[protocol.SearchDriverBrave] = provider
		credentials := protocol.DefaultSearchCredentials()
		credentials.Key = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(key)}
		call(caller, protocol.Command{Kind: "save_search_settings", Data: &protocol.SearchUpdate{
			Settings: settings, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{protocol.SearchDriverBrave: credentials},
		}})
		mu.Lock()
		keys[key] = users[caller].ID
		mu.Unlock()
	}
	search := func(caller protocol.NodeID, expected string) {
		t.Helper()
		input := protocol.DefaultSearchRequest()
		input.Query, input.Limit = "same personal query", 1
		output := call(caller, protocol.Command{Kind: "search_web", Data: &input})
		page, valid := dispatchPayload[protocol.SearchPage](output.Data)
		if !valid || len(page.Results) != 1 || page.Results[0].Title != expected {
			t.Fatalf("search used another user's credentials or cached response: %+v", output)
		}
	}
	for index, caller := range callers {
		save(caller, []string{"local-key", "first-key", "second-key"}[index])
	}
	for repeat := 0; repeat < 2; repeat++ {
		for index, caller := range callers {
			search(caller, []string{"local-key", "first-key", "second-key"}[index])
		}
	}
	save(callers[1], "updated-first-key")
	search(callers[1], "updated-first-key")
	search(callers[0], "local-key")
	search(callers[2], "second-key")
	mu.Lock()
	defer mu.Unlock()
	for _, key := range []string{"local-key", "first-key", "second-key", "updated-first-key"} {
		if calls[key] != 1 {
			t.Errorf("personal search cache for %s made %d requests, want 1", key, calls[key])
		}
	}
}

func TestNativeSearchSharesCooldownWithModelRequests(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		_, _ = io.Copy(io.Discard, request.Body)
		hits.Add(1)
		writer.Header().Set("Content-Type", "application/json")
		writer.Header().Set("Retry-After", "45")
		writer.WriteHeader(http.StatusTooManyRequests)
		_, _ = io.WriteString(writer, `{"error":{"code":"rate_limit_exceeded","message":"Rate limit reached"}}`)
	}))
	t.Cleanup(server.Close)

	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})

	secret := protocol.NewSecret("sk-native-search-shared")
	provider := protocol.ConversationProvider{
		ID:             protocol.NewProviderID(),
		Enabled:        true,
		Api:            protocol.ConversationModelApiResponses,
		Authentication: protocol.AuthenticationApiKey,
		Endpoint:       server.URL,
		Models:         []protocol.ConversationModel{{ID: "fixture-model"}},
	}
	binding := model.Binding{
		Provider: provider,
		Secret:   &secret,
		Config:   protocol.SessionConfig{Model: "fixture-model"},
	}
	cooldownKey := model.CredentialCooldownKey(binding)
	input := protocol.DefaultSearchRequest()
	input.Query, input.Limit = "shared cooldown test", 2
	req := protocol.NewRequest(node.ID(), protocol.Command{Kind: "search_web"})

	// 1. A 429 from nativeSearch must populate node.cooldowns for sibling model calls.
	if _, err := node.nativeSearch(ctx, req, binding, nil, input); err == nil {
		t.Fatal("expected HTTP 429 failure from nativeSearch")
	}
	if hits.Load() != 1 {
		t.Fatalf("nativeSearch hits = %d, want 1", hits.Load())
	}
	until, active := node.cooldowns.Until(cooldownKey, time.Now())
	if !active || time.Until(until) < 30*time.Second {
		t.Fatalf("nativeSearch did not record shared cooldown: active=%v remaining=%s", active, time.Until(until))
	}

	// 2. While cooled down, a subsequent nativeSearch waits rather than hitting the gateway.
	shortCtx, cancel := context.WithTimeout(ctx, 25*time.Millisecond)
	defer cancel()
	if _, err := node.nativeSearch(shortCtx, req, binding, nil, input); err == nil {
		t.Fatal("expected cooldown rejection/cancellation on short deadline")
	}
	if hits.Load() != 1 {
		t.Fatalf("nativeSearch hit gateway %d times during active shared cooldown", hits.Load())
	}
}
