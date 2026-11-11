package live

import (
	"slices"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Notifier observes the local controller and every connected host for the
// application lifetime, independently of frames and windows. It owns delivery
// cursors, never a business projection. Each new notice goes to one channel:
// the active window's toasts when a window takes it, otherwise a system
// notification. A notice neither channel took waits for the next focused
// window instead of being dropped.
type Notifier struct {
	controller         *client.Client
	prefs              *preferences.Store
	unsubscribe        func()
	unsubscribeNetwork func()
	mu                 sync.Mutex
	network            *Network
	peers              map[protocol.NodeID]*notifierPeer
	pending            []client.Notice
	// held are background notices the system channel did not take; sent are
	// those it was given, by notification ID, until it refuses one.
	held    []client.Notice
	sent    map[string]client.Notice
	seen    map[client.NoticeID]bool
	update  func(func())
	deliver func(id, title, body, session string)
	present func(*viewmodel.Notice) bool
	locale  func() string
	closed  bool
}

// notifierPeer is one remote host's controller. Its notices from before this
// application first read them are not news.
type notifierPeer struct {
	controller  *client.Client
	unsubscribe func()
	primed      bool
}

func NewNotifier(controller *client.Client, prefs *preferences.Store) *Notifier {
	n := &Notifier{controller: controller, prefs: prefs, seen: map[client.NoticeID]bool{}}
	for _, notice := range controller.Notices() {
		n.seen[notice.ID] = true
	}
	n.unsubscribe = controller.OnChange(n.observe)
	n.observe()
	return n
}

// SetNetwork adds the remote hosts: each connected host's notices reach the
// same channels as the local Node's.
func (n *Notifier) SetNetwork(network *Network) {
	n.mu.Lock()
	if n.closed || n.network != nil || network == nil {
		n.mu.Unlock()
		return
	}
	n.network = network
	n.mu.Unlock()
	unsubscribe := network.onChange(n.observe)
	n.mu.Lock()
	closed := n.closed
	if !closed {
		n.unsubscribeNetwork = unsubscribe
	}
	n.mu.Unlock()
	if closed {
		unsubscribe()
		return
	}
	n.observe()
}

// SetUpdate must schedule on the application UI thread even without a visible
// window. Bind it once, then retain this notifier until the application quits.
func (n *Notifier) SetUpdate(f func(func())) { n.mu.Lock(); n.update = f; n.mu.Unlock(); n.schedule() }
func (n *Notifier) SetNotify(f func(id, title, body, session string)) {
	n.mu.Lock()
	n.deliver = f
	n.mu.Unlock()
	n.schedule()
}

// SetPresent binds the in-app channel. It reports whether an active window
// took the notice; a notice it took gets no system notification.
func (n *Notifier) SetPresent(f func(*viewmodel.Notice) bool) {
	n.mu.Lock()
	n.present = f
	n.mu.Unlock()
}
func (n *Notifier) SetLocale(f func() string) { n.mu.Lock(); n.locale = f; n.mu.Unlock() }
func (n *Notifier) Close() {
	n.mu.Lock()
	if n.closed {
		n.mu.Unlock()
		return
	}
	n.closed = true
	release := []func(){n.unsubscribe, n.unsubscribeNetwork}
	for _, peer := range n.peers {
		release = append(release, peer.unsubscribe)
	}
	n.peers = nil
	n.mu.Unlock()
	for _, unsubscribe := range release {
		if unsubscribe != nil {
			unsubscribe()
		}
	}
}

// Undelivered returns a notice the system refused, as when notifications are
// denied: it waits for the next focused window.
func (n *Notifier) Undelivered(id string) {
	n.mu.Lock()
	if notice, sent := n.sent[id]; sent {
		delete(n.sent, id)
		n.hold(notice)
	}
	n.mu.Unlock()
	n.schedule()
}

// Returned tells that a window took the focus: the held notices show there.
func (n *Notifier) Returned() { n.schedule() }

// hold keeps the newest undelivered notices; mu is held.
func (n *Notifier) hold(notice client.Notice) {
	n.held = append(n.held, notice)
	if extra := len(n.held) - protocol.NotificationLimit; extra > 0 {
		n.held = slices.Delete(n.held, 0, extra)
	}
}

func (n *Notifier) observe() {
	var connected []networkPeer
	n.mu.Lock()
	network := n.network
	n.mu.Unlock()
	if network != nil {
		connected = network.snapshot()
	}
	n.mu.Lock()
	if n.closed {
		n.mu.Unlock()
		return
	}
	present := make(map[protocol.NodeID]bool, len(connected))
	for _, peer := range connected {
		if peer.controller == nil {
			continue
		}
		id := peer.address.ID
		present[id] = true
		current := n.peers[id]
		if current != nil && current.controller == peer.controller {
			continue
		}
		if current != nil {
			current.unsubscribe()
		}
		if n.peers == nil {
			n.peers = map[protocol.NodeID]*notifierPeer{}
		}
		n.peers[id] = &notifierPeer{controller: peer.controller, unsubscribe: peer.controller.OnChange(n.observe)}
	}
	for id, peer := range n.peers {
		if !present[id] {
			peer.unsubscribe()
			delete(n.peers, id)
		}
	}
	seen := map[client.NoticeID]bool{}
	unread := map[client.NoticeID]bool{}
	collect := func(controller *client.Client, announce bool) {
		for _, notice := range controller.Notices() {
			if announce && !n.seen[notice.ID] && !notice.Read {
				n.pending = append(n.pending, notice)
			}
			seen[notice.ID], unread[notice.ID] = true, !notice.Read
		}
	}
	collect(n.controller, true)
	for _, peer := range n.peers {
		ready := peer.controller.Connected()
		collect(peer.controller, peer.primed)
		peer.primed = peer.primed || ready
	}
	n.seen = seen
	// A notice read or dismissed elsewhere is no longer owed a toast.
	n.held = slices.DeleteFunc(n.held, func(notice client.Notice) bool { return !unread[notice.ID] })
	for id, notice := range n.sent {
		if !unread[notice.ID] {
			delete(n.sent, id)
		}
	}
	n.mu.Unlock()
	n.schedule()
}
func (n *Notifier) schedule() {
	n.mu.Lock()
	update, closed := n.update, n.closed
	n.mu.Unlock()
	if update != nil && !closed {
		update(n.flush)
	}
}
func (n *Notifier) flush() {
	n.mu.Lock()
	if n.closed || n.deliver == nil {
		n.mu.Unlock()
		return
	}
	held, list, deliver, present, locale := n.held, n.pending, n.deliver, n.present, n.locale
	n.held, n.pending = nil, nil
	n.mu.Unlock()
	flags := n.prefs.Data.Notifications
	for index, notice := range append(held, list...) {
		projected := Notices([]client.Notice{notice})[0]
		enabled := true
		switch notice.Kind {
		case "approval", "input":
			enabled = flags[1]
		case "completed":
			enabled = flags[2]
		case "failed":
			enabled = flags[3]
		}
		if !enabled {
			continue
		}
		if present != nil && present(projected) {
			continue
		}
		// The first flag is the system channel's own switch. A held notice
		// was already refused there; both wait for a window.
		if index < len(held) || !flags[0] {
			n.mu.Lock()
			n.hold(notice)
			n.mu.Unlock()
			continue
		}
		language := "en"
		if n.prefs.Data.Language != nil && *n.prefs.Data.Language == "chinese" {
			language = "zh-CN"
		}
		if locale != nil {
			language = i18n.Match(locale())
		}
		title, body := noticeText(projected, language)
		n.mu.Lock()
		if n.sent == nil {
			n.sent = map[string]client.Notice{}
		}
		n.sent[projected.ID] = notice
		n.mu.Unlock()
		deliver(projected.ID, title, body, projected.Session)
	}
}

// noticeText is what a notice says in a language. A package's heading goes
// above the notice's own title, which then leads the body.
func noticeText(notice *viewmodel.Notice, language string) (title, body string) {
	title, body = notice.Title, notice.Message
	if heading := shell.NoticeHeading(notice, language); heading != "" {
		parts := []string{}
		for _, part := range []string{notice.Title, notice.Message} {
			if strings.TrimSpace(part) != "" {
				parts = append(parts, part)
			}
		}
		return heading, strings.Join(parts, "\n")
	}
	if notice.TitleKey != "" {
		title = i18n.In(language, notice.TitleKey)
	}
	return title, body
}
