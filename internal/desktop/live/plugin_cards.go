package live

import (
	"context"
	"encoding/json"
	"reflect"
	"slices"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginCardKey struct {
	node    protocol.NodeID
	receipt protocol.PluginCardReceipt
}

type pluginCardCallKey struct {
	node    protocol.NodeID
	session string
	call    string
}

type pluginCardState struct {
	cards      map[pluginCardKey]*pluginCardInstance
	calls      map[pluginCardCallKey][]pluginCardKey
	generation uint64
	closed     bool
}

type pluginCardInstance struct {
	key         pluginCardKey
	owner       *Runtime
	summary     protocol.PluginCardSummary
	info        protocol.PluginInfo
	declaration protocol.PluginCardDeclaration
	generation  uint64
	locale      string
	ctx         context.Context
	cancel      context.CancelFunc
	host        *desktopplugins.CardHost
	runtime     *desktopplugins.Runtime
	mounted     *desktopplugins.Mounted
	loading     bool
	attempted   bool
	failure     error
	reported    bool
	visible     bool
	probed      bool
	lastPaint   time.Time
	login       pluginCardAuthorization
	popup       *pluginCardBrowserWindow
	settings    bool
	browser     bool
	mu          sync.Mutex
	reference   protocol.PluginReference
}

func (r *Runtime) initPluginCards() *pluginCardState {
	root := r.localRuntime()
	if root.pluginCards == nil {
		root.pluginCards = &pluginCardState{
			cards: map[pluginCardKey]*pluginCardInstance{}, calls: map[pluginCardCallKey][]pluginCardKey{},
		}
	}
	return root.pluginCards
}

func (r *Runtime) bindPluginCards(cv *viewmodel.Conversation, callID string, loaded protocol.ToolDetail) {
	if cv == nil {
		return
	}
	root, state := r.localRuntime(), r.initPluginCards()
	if state.closed {
		return
	}
	call := pluginCardCallKey{node: r.controller.Target(), session: cv.ID, call: callID}
	var keys []pluginCardKey
	if loaded.Content != nil {
		for index, block := range loaded.Content.Blocks {
			if block.Kind != "card" {
				continue
			}
			summary, valid := detail[protocol.PluginCardSummary](block.Data)
			if !valid || summary.Node != call.node || summary.Receipt.Session.String() != cv.ID ||
				int(summary.Receipt.Block) != index || !nodeplugins.CardSummaryValid(summary) {
				continue
			}
			key := pluginCardKey{node: summary.Node, receipt: summary.Receipt}
			card := state.cards[key]
			if card != nil && !reflect.DeepEqual(card.summary, summary) {
				root.suspendPluginCard(card)
				delete(state.cards, key)
				card = nil
			}
			if card == nil {
				body, err := json.Marshal(summary)
				if err != nil || json.Unmarshal(body, &summary) != nil {
					continue
				}
				card = &pluginCardInstance{key: key, owner: r, summary: summary, reference: summary.Package}
				state.cards[key] = card
			}
			keys = append(keys, key)
		}
	}
	for _, previous := range state.calls[call] {
		if !slices.Contains(keys, previous) {
			if card := state.cards[previous]; card != nil {
				root.suspendPluginCard(card)
				delete(state.cards, previous)
			}
		}
	}
	if len(keys) == 0 {
		delete(state.calls, call)
		root.App.Messages.SetToolCard(cv.ID, callID, nil)
		return
	}
	state.calls[call] = keys
	root.App.Messages.SetToolCard(cv.ID, callID, func(view *ui.Context) {
		current := root.pluginCards
		if current == nil || current.closed || !reflect.DeepEqual(current.calls[call], keys) {
			return
		}
		ui.Column(view).FillWidth().MinWidth(0).Gap(8).Children(func() {
			for _, key := range keys {
				if card := current.cards[key]; card != nil && card.owner == r {
					root.renderPluginCard(view.Key(key.receipt.Call.String()).Key(key.receipt.Block), card)
				}
			}
		})
	})
}

func (r *Runtime) pluginCardEligible(card *pluginCardInstance) bool {
	root := r.localRuntime()
	if root.pluginCards == nil || root.pluginCards.closed || root.pluginCards.cards[card.key] != card ||
		card.owner.ctx.Err() != nil || !card.owner.controller.Connected() || card.summary.Node != card.owner.controller.Target() {
		return false
	}
	session, exists := card.owner.session(card.summary.Receipt.Session.String())
	if !exists || session.Delegation != nil || session.Worktree != card.summary.Worktree {
		return false
	}
	history, exists := card.owner.controller.Conversation(card.summary.Receipt.Session)
	if !exists || history.Removed {
		return false
	}
	for _, summary := range card.owner.snapshot().Plugins {
		if summary.Name == card.summary.Package.Name && summary.Digest == card.summary.Package.Digest {
			card.updateReference(packageReference(summary))
			break
		}
	}
	var latest *protocol.ConversationRun
	for index := range history.Snapshot.Page.Runs {
		run := &history.Snapshot.Page.Runs[index]
		if latest == nil || run.Sequence > latest.Sequence {
			latest = run
		}
	}
	return latest != nil && latest.Turn == card.summary.Receipt.Turn && latest.Origin == nil &&
		slices.Contains([]protocol.ConversationStatus{protocol.ConversationStatusRunning, protocol.ConversationStatusCompleted, protocol.ConversationStatusFailed}, latest.Status)
}

func (r *Runtime) pluginCardLive(card *pluginCardInstance, generation uint64) bool {
	return card.generation == generation && card.ctx != nil && card.ctx.Err() == nil && r.pluginCardEligible(card)
}

func (r *Runtime) renderPluginCard(view *ui.Context, card *pluginCardInstance) {
	box := ui.Column(view).FillWidth().MinWidth(0).Gap(8)
	eligible := r.pluginCardEligible(card)
	card.visible = box.InView() && eligible
	if card.visible {
		card.lastPaint = time.Now()
		r.ensurePluginCard(view, card)
	}
	if !card.probed {
		card.probed = true
		r.wake()
	}
	box.Children(func() {
		if eligible && card.mounted != nil {
			if _, err := card.mounted.Render(view.Key(card.generation)); err != nil {
				r.failPluginCard(view, card, err, "plugin_callback_failed")
			}
		}
		if card.mounted == nil || !eligible {
			palette := theme.Of(view)
			ui.Box(view).FillWidth().MinWidth(0).Padding(12).Radius(palette.Radius).Background(palette.Muted).
				Children(func() {
					ui.Text(view, pluginLabel(card.summary.Fallback, view.Locale())).TextColor(palette.MutedForeground)
				})
		}
	})
}

func (r *Runtime) pluginCardsFrame(view *ui.Context) {
	if r.parent != nil || r.pluginCards == nil || r.pluginCards.closed {
		return
	}
	for _, card := range r.pluginCards.cards {
		card.visible = false
		if !r.pluginCardEligible(card) || card.locale != "" && card.locale != r.packageLocale() {
			r.suspendPluginCard(card)
			continue
		}
		if card.mounted != nil {
			if err := card.mounted.Pump(); err != nil {
				r.failPluginCard(view, card, err, "plugin_callback_failed")
			}
		}
	}
}

func (r *Runtime) pluginCardsAfterFrame() {
	if r.parent != nil || r.pluginCards == nil || r.pluginCards.closed {
		return
	}
	now := time.Now()
	for _, card := range r.pluginCards.cards {
		if !card.visible && card.cancel != nil && now.Sub(card.lastPaint) >= time.Second && !card.login.keepsAlive(now) {
			r.suspendPluginCard(card)
		}
	}
}

func (r *Runtime) closePluginCards() {
	root := r.localRuntime()
	if root.pluginCards == nil {
		return
	}
	if r.parent == nil {
		root.pluginCards.closed = true
	}
	for key, card := range root.pluginCards.cards {
		if r.parent == nil || card.owner == r {
			root.suspendPluginCard(card)
			delete(root.pluginCards.cards, key)
		}
	}
	for key := range root.pluginCards.calls {
		if r.parent == nil || key.node == r.controller.Target() {
			root.App.Messages.SetToolCard(key.session, key.call, nil)
			delete(root.pluginCards.calls, key)
		}
	}
}

func (r *Runtime) suspendPluginCard(card *pluginCardInstance) {
	if card.cancel != nil {
		card.cancel()
		card.cancel = nil
	}
	card.login.suspend()
	if card.runtime != nil {
		_ = card.runtime.Close()
		card.runtime, card.mounted = nil, nil
	}
	if card.host != nil {
		_ = card.host.Close()
		card.host = nil
	}
	if card.popup != nil {
		card.popup.close()
		card.popup = nil
	}
	card.loading, card.attempted, card.visible = false, false, false
	card.settings, card.browser = false, false
}

func (r *Runtime) failPluginCard(view *ui.Context, card *pluginCardInstance, err error, title string) {
	card.failure = err
	r.suspendPluginCard(card)
	card.attempted = true
	if view == nil || card.reported {
		return
	}
	card.reported = true
	view.AddToast(ui.Toast{
		Type: "error", Title: i18n.T(view, title), Description: err.Error(), Timeout: -1,
		Action: i18n.T(view, "chat_retry"), OnAction: func() {
			r.enqueue(func(*ui.Context) {
				if r.pluginCardEligible(card) {
					card.attempted, card.reported, card.failure = false, false, nil
				}
			})
		},
	})
}

func (r *Runtime) pluginCardToast(view *ui.Context, message string, failed bool) {
	if failed {
		kit.Toast(view, kit.Failed, message)
		return
	}
	view.AddToast(ui.Toast{Type: "info", Title: message, Timeout: kit.ToastDuration(view)})
}
