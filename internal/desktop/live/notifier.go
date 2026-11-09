package live

import (
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
)

// Notifier observes one controller for the application lifetime, independently
// of frames and windows. It owns delivery cursors, never a business projection.
type Notifier struct {
	controller  *client.Client
	prefs       *preferences.Store
	unsubscribe func()
	mu          sync.Mutex
	pending     []client.Notice
	seen        map[client.NoticeID]bool
	update      func(func())
	deliver     func(id, title, body, session string)
	locale      func() string
	closed      bool
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

// SetUpdate must schedule on the application UI thread even without a visible
// window. Bind it once, then retain this notifier until the application quits.
func (n *Notifier) SetUpdate(f func(func())) { n.mu.Lock(); n.update = f; n.mu.Unlock(); n.schedule() }
func (n *Notifier) SetNotify(f func(id, title, body, session string)) {
	n.mu.Lock()
	n.deliver = f
	n.mu.Unlock()
	n.schedule()
}
func (n *Notifier) SetLocale(f func() string) { n.mu.Lock(); n.locale = f; n.mu.Unlock() }
func (n *Notifier) Close() {
	n.mu.Lock()
	if n.closed {
		n.mu.Unlock()
		return
	}
	n.closed = true
	n.mu.Unlock()
	n.unsubscribe()
}

func (n *Notifier) observe() {
	list := n.controller.Notices()
	n.mu.Lock()
	if n.closed {
		n.mu.Unlock()
		return
	}
	seen := make(map[client.NoticeID]bool, len(list))
	for _, notice := range list {
		if !n.seen[notice.ID] && !notice.Read {
			n.pending = append(n.pending, notice)
		}
		seen[notice.ID] = true
	}
	n.seen = seen
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
	list, deliver, locale := n.pending, n.deliver, n.locale
	n.pending = nil
	n.mu.Unlock()
	for _, notice := range list {
		flags := n.prefs.Data.Notifications
		if !flags[0] {
			continue
		}
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
		if projected.TitleKey != "" {
			language := "en"
			if n.prefs.Data.Language != nil && *n.prefs.Data.Language == "chinese" {
				language = "zh-CN"
			}
			if locale != nil {
				language = i18n.Match(locale())
			}
			projected.Title = i18n.In(language, projected.TitleKey)
		}
		if enabled {
			deliver(projected.ID, projected.Title, projected.Message, projected.Session)
		}
	}
}
