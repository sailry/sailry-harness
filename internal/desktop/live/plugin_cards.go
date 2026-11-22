package live

import (
	"context"
	"encoding/json"
	"reflect"
	"slices"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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
	visits      int
	probed      bool
	lastPaint   time.Time
	login       pluginCardAuthorization
	popup       *pluginCardBrowserWindow
	settings    bool
	browser     bool
	mu          sync.Mutex
	reference   protocol.PluginReference
}

// A failed card retries when its last presentation leaves and the user opens
// it again, even after its recovery toast expires. Other panes may still show
// the same card, so one presentation leaving must not reset their attempt.
type pluginCardVisit struct{ card *pluginCardInstance }

func (visit *pluginCardVisit) Close() error {
	card := visit.card
	card.visits--
	if card.visits == 0 && card.failure != nil {
		card.attempted, card.reported, card.failure = false, false, nil
	}
	return nil
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

func toolPluginCards(content *protocol.ToolContent) []protocol.PluginCardSummary {
	var cards []protocol.PluginCardSummary
	if content != nil {
		for index, block := range content.Blocks {
			if block.Kind == "card" {
				if summary, valid := detail[protocol.PluginCardSummary](block.Data); valid && int(summary.Receipt.Block) == index && nodeplugins.CardSummaryValid(summary) {
					cards = append(cards, summary)
				}
			}
		}
	}
	return cards
}

func (r *Runtime) bindPluginCards(session, callID string, summaries []protocol.PluginCardSummary) {
	root := r.localRuntime()
	if len(summaries) == 0 && root.pluginCards == nil {
		return
	}
	state := r.initPluginCards()
	if state.closed {
		return
	}
	call := pluginCardCallKey{node: r.controller.Target(), session: session, call: callID}
	var keys []pluginCardKey
	for _, summary := range summaries {
		if summary.Node != call.node || summary.Receipt.Session.String() != session {
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
		root.App.Messages.SetToolCard(session, callID, nil)
		return
	}
	state.calls[call] = keys
	root.App.Messages.SetToolCard(session, callID, func(view *ui.Context) {
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
	status, exists := card.owner.controller.LatestLocalTurnStatus(card.summary.Receipt.Session, card.summary.Receipt.Turn)
	if !exists || !slices.Contains([]protocol.ConversationStatus{protocol.ConversationStatusRunning, protocol.ConversationStatusCompleted, protocol.ConversationStatusFailed}, status) {
		return false
	}
	for _, summary := range card.owner.snapshot().Plugins {
		if summary.Name != card.summary.Package.Name {
			continue
		}
		if !summary.Enabled || summary.Compatibility.Kind != "compatible" || summary.Digest != card.summary.Package.Digest {
			return false
		}
		card.updateReference(packageReference(summary))
		return true
	}
	return false
}

func (r *Runtime) pluginCardLive(card *pluginCardInstance, generation uint64) bool {
	return card.generation == generation && card.ctx != nil && card.ctx.Err() == nil && r.pluginCardEligible(card)
}

func (r *Runtime) renderPluginCard(view *ui.Context, card *pluginCardInstance) {
	box := ui.Column(view).FillWidth().MinWidth(0).Gap(8)
	ui.Local(box, card, func() pluginCardVisit {
		card.visits++
		return pluginCardVisit{card: card}
	})
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
			if card.ctx != nil || card.attempted {
				r.suspendPluginCard(card)
			}
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

func (r *Runtime) pruneConversationCards(session protocol.SessionID, turns []conversation.Turn) {
	root := r.localRuntime()
	state := root.pluginCards
	if state == nil || len(state.calls) == 0 {
		return
	}
	node, id := r.controller.Target(), session.String()
	var retained map[string]bool
	for call, keys := range state.calls {
		if call.node != node || call.session != id {
			continue
		}
		if retained == nil {
			retained = make(map[string]bool)
			for _, turn := range turns {
				for _, block := range turn.Reply.Blocks {
					for _, tool := range block.Calls {
						if tool.Load != nil {
							retained[tool.ID] = true
						}
					}
				}
			}
		}
		if retained[call.call] {
			continue
		}
		if slices.ContainsFunc(keys, func(key pluginCardKey) bool {
			card := state.cards[key]
			return card != nil && (card.login.request != nil || card.login.attempt != nil || card.login.pending)
		}) {
			continue
		}
		for _, key := range keys {
			if card := state.cards[key]; card != nil {
				root.suspendPluginCard(card)
				delete(state.cards, key)
			}
		}
		root.App.Messages.SetToolCard(id, call.call, nil)
		delete(state.calls, call)
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
	}
	card.ctx, card.cancel = nil, nil
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
	// A response can fail after the last presentation was unmounted but before
	// its idle suspension ran. It must not block the next explicit visit.
	card.attempted = card.visits > 0
	if view == nil || card.reported {
		return
	}
	card.reported = true
	kit.Error(view, i18n.T(view, title), err.Error())
}

func (r *Runtime) pluginCardToast(view *ui.Context, message string, failed bool) {
	if failed {
		kit.Toast(view, kit.Failed, message)
		return
	}
	view.AddToast(ui.Toast{Type: "info", Title: message, Timeout: kit.ToastDuration(view)})
}
