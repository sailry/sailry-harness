package node

import (
	"context"
	"database/sql"
	"sync"

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
	mu     sync.Mutex
	active map[webOperationKey]context.CancelFunc
	fetch  *web.Fetcher
	search *web.Searcher
}

func (node *Node) initWeb(ctx context.Context, options Options) error {
	node.web = &workspaceWeb{active: map[webOperationKey]context.CancelFunc{}, fetch: web.NewFetcher(options.Fetch), search: web.NewSearcher(options.Search)}
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		settings, err := store.ReadSearchSettings(ctx, db)
		if err == nil {
			node.web.search.Configure(settings)
		}
		return err
	})
}
func (node *Node) webContext(caller protocol.NodeID, request protocol.Request, search bool) (context.Context, func(), error) {
	parent := node.ctx
	finish := func() {}
	if request.Plugin != nil && request.Plugin.Turn != nil {
		var ok bool
		parent, finish, ok = node.agent.BeginOperation(*request.Plugin.Turn)
		if !ok {
			return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "web operation turn is no longer running"}
		}
	}
	ctx, cancel := context.WithCancel(parent)
	key := webOperationKey{caller, request.ID, search}
	node.web.mu.Lock()
	if _, exists := node.web.active[key]; exists {
		node.web.mu.Unlock()
		cancel()
		finish()
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "web request is already running"}
	}
	node.web.active[key] = cancel
	node.web.mu.Unlock()
	return ctx, func() { cancel(); node.web.mu.Lock(); delete(node.web.active, key); node.web.mu.Unlock(); finish() }, nil
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
		effect.after = func() {
			_ = node.profile.Store.Node.Read(context.WithoutCancel(node.ctx), func(db *sql.Conn) error {
				value, err := store.ReadSearchSettings(context.WithoutCancel(node.ctx), db)
				if err == nil {
					node.web.search.Configure(value)
				}
				return err
			})
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
	operation, release, err := node.webContext(caller, request, search)
	if err != nil {
		return protocol.Output{}, err
	}
	defer release()
	plan, err := node.searchPlan(operation, request)
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
	page, err := node.web.search.SearchFresh(operation, plan, input, fresh)
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
func (node *Node) searchPlan(ctx context.Context, request protocol.Request) (web.SearchPlan, error) {
	var plan web.SearchPlan
	var provider protocol.ConversationProvider
	var selection *protocol.SearchModelConfig
	var modelErr error
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		plan, err = store.ReadWebSearchPlan(ctx, db)
		if err != nil {
			return err
		}
		selection = plan.Settings.Providers[protocol.SearchDriverModel].Model
		if selection != nil {
			provider, modelErr = store.CaptureSearchProvider(ctx, db, *selection)
		}
		return nil
	})
	if err != nil {
		return plan, err
	}
	plan.Model = func(ctx context.Context, input protocol.SearchRequest) (protocol.SearchPage, error) {
		if selection == nil {
			return protocol.SearchPage{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "search model is not configured"}
		}
		if modelErr != nil {
			return protocol.SearchPage{}, modelErr
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
	return plan, nil
}
