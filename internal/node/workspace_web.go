package node

import (
	"context"
	"database/sql"
	"runtime"
	"sync"
	"time"
	"weak"

	"github.com/sailry/sailry-harness/internal/memory"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type webOperationKey struct {
	caller  protocol.NodeID
	request protocol.RequestID
	search  bool
}
type workspaceWeb struct {
	mu         sync.Mutex
	active     map[webOperationKey]context.CancelFunc
	fetch      *web.Fetcher
	search     map[protocol.UserID]*web.Searcher
	options    web.SearchOptions
	stopMemory func()
}

func (node *Node) initWeb(ctx context.Context, options Options) error {
	ws := &workspaceWeb{active: map[webOperationKey]context.CancelFunc{}, fetch: web.NewFetcher(options.Fetch), search: map[protocol.UserID]*web.Searcher{}, options: options.Search}
	weakWS := weak.Make(ws)
	ws.stopMemory = memory.Default().Register("node.web_search", func(level memory.Level) {
		if level != memory.Pressure {
			return
		}
		if owner := weakWS.Value(); owner != nil {
			owner.mu.Lock()
			for _, searcher := range owner.search {
				searcher.ClearResults()
			}
			owner.mu.Unlock()
		}
	})
	runtime.AddCleanup(ws, func(stop func()) { stop() }, ws.stopMemory)
	node.web = ws
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		settings, err := store.ReadSearchSettings(ctx, db)
		if err != nil {
			return err
		}
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		node.web.searcher(user.ID, settings)
		return nil
	})
}

func (workspace *workspaceWeb) searcher(user protocol.UserID, settings protocol.SearchSettings) *web.Searcher {
	workspace.mu.Lock()
	defer workspace.mu.Unlock()
	searcher := workspace.search[user]
	if searcher == nil {
		searcher = web.NewSearcher(workspace.options)
		workspace.search[user] = searcher
	}
	// Driver cooldowns and result caches belong to the configured user.
	searcher.Configure(settings)
	return searcher
}

func (node *Node) webContext(ctx context.Context, caller protocol.NodeID, request protocol.Request, search bool) (context.Context, func(), error) {
	parent, finish, err := node.beginPluginExecution(ctx, request)
	if err != nil {
		return nil, nil, err
	}
	ctx, cancel := context.WithCancel(parent)
	stop := context.AfterFunc(node.ctx, cancel)
	key := webOperationKey{caller, request.ID, search}
	node.web.mu.Lock()
	if _, exists := node.web.active[key]; exists {
		node.web.mu.Unlock()
		stop()
		cancel()
		finish()
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "web request is already running"}
	}
	node.web.active[key] = cancel
	node.web.mu.Unlock()
	return ctx, func() {
		stop()
		cancel()
		node.web.mu.Lock()
		delete(node.web.active, key)
		node.web.mu.Unlock()
		finish()
	}, nil
}
func (node *Node) cancelWeb(caller protocol.NodeID, id protocol.RequestID, search bool) {
	node.web.mu.Lock()
	cancel := node.web.active[webOperationKey{caller, id, search}]
	node.web.mu.Unlock()
	if cancel != nil {
		cancel()
	}
}
func (node *Node) workspaceWebIO(ctx context.Context, caller protocol.NodeID, request protocol.Request, effect *workspaceCompletion) (protocol.Output, error) {
	switch request.Command.Kind {
	case "read_search_settings":
		var value protocol.SearchSettings
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; value, err = store.ReadSearchSettings(ctx, db); return err })
		return protocol.Output{Kind: "search_settings", Data: value}, err
	case "save_search_settings":
		input := request.Command.Data.(*protocol.SearchUpdate)
		effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
			return store.SaveSearchSettings(ctx, db, request, *input)
		}
		return protocol.Output{}, nil
	case "cancel_web_fetch":
		id := request.Command.Data.(*protocol.CommandCancelWebFetchPayload).Request
		node.cancelWeb(caller, id, false)
		return protocol.Output{Kind: "web_fetch_cancelled", Data: protocol.OutputWebFetchCancelledPayload{Request: id}}, nil
	case "cancel_web_search":
		id := request.Command.Data.(*protocol.CommandCancelWebSearchPayload).Request
		node.cancelWeb(caller, id, true)
		return protocol.Output{Kind: "web_search_cancelled", Data: protocol.OutputWebSearchCancelledPayload{Request: id}}, nil
	}
	search := request.Command.Kind != "fetch_web"
	if request.Command.Kind == "test_search_driver" && !store.SearchSettingsController(request) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "search driver tests require a controller"}
	}
	operation, release, err := node.webContext(ctx, caller, request, search)
	if err != nil {
		return protocol.Output{}, err
	}
	defer release()
	plan, user, err := node.searchPlan(operation, request)
	if err != nil {
		return protocol.Output{}, err
	}
	if !search {
		input := *request.Command.Data.(*protocol.WebRequest)
		page, err := node.web.fetch.FetchWith(operation, input, node.searchRenderer(plan, input.Offset == 0))
		return protocol.Output{Kind: "web_page", Data: page}, err
	}
	var input protocol.SearchRequest
	fresh := request.Command.Kind == "test_search_driver"
	if fresh {
		driver := request.Command.Data.(*protocol.CommandTestSearchDriverPayload).Driver
		plan.Settings.Driver = &protocol.SearchSelection{Kind: "driver", Data: driver}
		plan.Settings.Fallback = false
		config := plan.Settings.Providers[driver]
		config.Enabled = true
		plan.Settings.Providers[driver] = config
		input = protocol.DefaultSearchRequest()
		input.Query = "Sailry"
		input.Limit = 1
	} else {
		input = *request.Command.Data.(*protocol.SearchRequest)
	}
	metric, started := node.telemetry.Capture(), time.Now()
	page, err := node.web.searcher(user, plan.Settings).SearchFresh(operation, plan, input, fresh)
	metric.Record(searchObservation(plan, page, err, time.Since(started)))
	return protocol.Output{Kind: "search_page", Data: page}, err
}
func (node *Node) searchRenderer(plan web.SearchPlan, enabled bool) *web.Renderer {
	config := plan.Settings.Providers[protocol.SearchDriverTinyfish]
	auth := plan.Credentials[protocol.SearchDriverTinyfish]
	if !enabled || !config.Enabled || auth.Key == nil {
		return nil
	}
	return &web.Renderer{Key: *auth.Key}
}
func (node *Node) searchPlan(ctx context.Context, request protocol.Request) (web.SearchPlan, protocol.UserID, error) {
	var plan web.SearchPlan
	var user protocol.User
	var provider protocol.ConversationProvider
	var selection *protocol.SearchModelConfig
	var modelErr error
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		user, err = store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		plan, err = store.ReadWebSearchPlan(ctx, db)
		if err != nil {
			return err
		}
		model := plan.Settings.Providers[protocol.SearchDriverModel]
		// Only a search that can reach model search resolves its model; a
		// driver test enables the driver it tests.
		if request.Command.Kind == "fetch_web" || !model.Enabled && request.Command.Kind != "test_search_driver" {
			return nil
		}
		selection = model.Model
		// Without a chosen search model, model search follows the calling
		// conversation's current model.
		if selection == nil && request.Plugin != nil && request.Plugin.Session != nil {
			session, err := store.ReadSession(ctx, db, *request.Plugin.Session)
			if err != nil {
				modelErr = err
			} else {
				selection = &protocol.SearchModelConfig{Provider: session.Config.Provider, Model: session.Config.Model}
			}
		}
		if selection != nil {
			provider, modelErr = store.CaptureSearchProvider(ctx, db, *selection)
		}
		return nil
	})
	if err != nil {
		return plan, user.ID, err
	}
	plan.Model = func(ctx context.Context, input protocol.SearchRequest) (protocol.SearchPage, error) {
		if modelErr != nil {
			return protocol.SearchPage{}, modelErr
		}
		if selection == nil {
			return protocol.SearchPage{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "model search needs a conversation or a chosen model"}
		}
		session := protocol.NewSessionID()
		var turn *protocol.TurnID
		if request.Plugin != nil && request.Plugin.Session != nil {
			session = *request.Plugin.Session
			turn = request.Plugin.Turn
		}
		var binding model.Binding
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			binding, err = store.ResolveSearchBinding(ctx, db, node.ID(), provider, selection.Model, session)
			return err
		})
		if err != nil {
			return protocol.SearchPage{}, searchModelFault(err)
		}
		return node.nativeSearch(ctx, request, binding, turn, input)
	}
	return plan, user.ID, nil
}
