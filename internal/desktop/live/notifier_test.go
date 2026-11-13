package live

import (
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNotifierDelivery(t *testing.T) {
	node := protocol.NodeID{1}
	plugin := client.Notice{ID: client.NoticeID{Node: node, Cursor: 3, Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NotificationID{3}}}, Kind: "notification", NotificationKind: protocol.NotificationKindInfo, Title: "Index", Message: "Rebuilt"}
	// channels binds a notifier whose window is focused only while *focused.
	channels := func(n *Notifier, focused *bool) (toasts, system *[]string) {
		toasts, system = new([]string), new([]string)
		n.SetNotify(func(id, _, _, _ string) { *system = append(*system, id) })
		n.SetPresent(func(notice *viewmodel.Notice) bool {
			if *focused {
				*toasts = append(*toasts, notice.ID)
			}
			return *focused
		})
		n.SetUpdate(func(f func()) { f() })
		return toasts, system
	}
	id := noticeID(plugin)
	t.Run("a notice the system refuses shows once in the next focused window", func(t *testing.T) {
		focused := false
		n := &Notifier{prefs: &preferences.Store{Data: preferences.Default()}, seen: map[client.NoticeID]bool{}, pending: []client.Notice{plugin}}
		toasts, system := channels(n, &focused)
		if !slices.Equal(*system, []string{id}) || len(*toasts) != 0 {
			t.Fatalf("background delivery: toasts %v, system %v", *toasts, *system)
		}
		n.Undelivered(id)
		n.Returned()
		if len(*toasts) != 0 || len(*system) != 1 {
			t.Fatalf("a refused notice left without a window: toasts %v, system %v", *toasts, *system)
		}
		focused = true
		n.Returned()
		n.Returned()
		if !slices.Equal(*toasts, []string{id}) || len(*system) != 1 {
			t.Fatalf("after the return: toasts %v, system %v", *toasts, *system)
		}
	})
	t.Run("a notice the system took is not shown again", func(t *testing.T) {
		focused := false
		n := &Notifier{prefs: &preferences.Store{Data: preferences.Default()}, seen: map[client.NoticeID]bool{}, pending: []client.Notice{plugin}}
		toasts, system := channels(n, &focused)
		focused = true
		n.Returned()
		if len(*toasts) != 0 || !slices.Equal(*system, []string{id}) {
			t.Fatalf("toasts %v, system %v", *toasts, *system)
		}
	})
	t.Run("with the system switch off a background notice waits for a window", func(t *testing.T) {
		prefs := &preferences.Store{Data: preferences.Default()}
		prefs.Data.Notifications[0] = false
		focused := false
		n := &Notifier{prefs: prefs, seen: map[client.NoticeID]bool{}, pending: []client.Notice{plugin}}
		toasts, system := channels(n, &focused)
		focused = true
		n.Returned()
		if !slices.Equal(*toasts, []string{id}) || len(*system) != 0 {
			t.Fatalf("toasts %v, system %v", *toasts, *system)
		}
	})
	t.Run("a category switched off is not kept for the return", func(t *testing.T) {
		prefs := &preferences.Store{Data: preferences.Default()}
		prefs.Data.Notifications[0], prefs.Data.Notifications[3] = false, false
		session := protocol.SessionID{7}
		failed := client.Notice{ID: client.NoticeID{Node: node, Cursor: 1, Target: client.NoticeTarget{Kind: "session", Session: session}}, Kind: "failed", Session: &session}
		focused := false
		n := &Notifier{prefs: prefs, seen: map[client.NoticeID]bool{}, pending: []client.Notice{failed}}
		toasts, system := channels(n, &focused)
		focused = true
		n.Returned()
		if len(*toasts) != 0 || len(*system) != 0 {
			t.Fatalf("toasts %v, system %v", *toasts, *system)
		}
	})
	t.Run("a connected host's new notices are delivered and its earlier ones are not", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			start := func(snapshot protocol.Snapshot) (*pluginViewTestNode, *client.Client) {
				source := &pluginViewTestNode{snapshot: snapshot}
				controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
				if err := controller.Start(t.Context()); err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = controller.Close() })
				return source, controller
			}
			notification := func(id byte, sequence uint64) protocol.NotificationNotice {
				value := protocol.DefaultNotificationNotice()
				value.ID, value.Sequence, value.TimestampMs = protocol.NotificationID{id}, sequence, int64(sequence)
				value.Content.Title, value.Content.Kind = "Remote", protocol.NotificationKindInfo
				return value
			}
			home := protocol.DefaultSnapshot()
			home.Node = node
			_, local := start(home)
			away := protocol.DefaultSnapshot()
			away.Node, away.Cursor = protocol.NodeID{2}, 1
			away.Notifications = []protocol.NotificationNotice{notification(1, 1)}
			source, remote := start(away)
			network := &Network{peers: map[protocol.NodeID]*networkPeer{away.Node: {controller: remote}}, callbacks: map[uint64]func(){}}
			network.peers[away.Node].address.ID = away.Node
			n := NewNotifier(local, &preferences.Store{Data: preferences.Default()})
			t.Cleanup(n.Close)
			n.SetNetwork(network)
			focused := false
			toasts, system := channels(n, &focused)
			if len(*toasts) != 0 || len(*system) != 0 {
				t.Fatalf("a host's earlier notice was news: toasts %v, system %v", *toasts, *system)
			}
			fresh := notification(2, 2)
			source.snapshot.Cursor, source.snapshot.Notifications = 2, []protocol.NotificationNotice{fresh, notification(1, 1)}
			if err := remote.Refresh(t.Context()); err != nil {
				t.Fatal(err)
			}
			synctest.Wait()
			if !slices.Equal(*system, []string{fresh.ID.String()}) || len(*toasts) != 0 {
				t.Fatalf("a remote host's new notice: toasts %v, system %v", *toasts, *system)
			}
			// A host that left is no longer observed.
			network.mu.Lock()
			delete(network.peers, away.Node)
			network.mu.Unlock()
			network.changed()
			source.snapshot.Cursor, source.snapshot.Notifications = 3, []protocol.NotificationNotice{notification(3, 3), fresh, notification(1, 1)}
			if err := remote.Refresh(t.Context()); err != nil {
				t.Fatal(err)
			}
			synctest.Wait()
			if len(*system) != 1 {
				t.Fatalf("a departed host still notified: %v", *system)
			}
		})
	})
	t.Run("a package heading titles the notice and its own title leads the body", func(t *testing.T) {
		reminder := &viewmodel.Notice{ID: "reminder", Kind: viewmodel.NoticeInfo, Title: "Call the bank", Message: "Before noon",
			Presentation: &protocol.NotificationPresentation{Heading: "Reminder", Locales: map[string]string{"zh-CN": "提醒"}, Persistent: true}}
		for language, heading := range map[string]string{"en": "Reminder", "zh-CN": "提醒", "ja": "Reminder"} {
			if title, body := noticeText(reminder, language); title != heading || body != "Call the bank\nBefore noon" {
				t.Fatalf("%s: %q, %q", language, title, body)
			}
		}
		// The application's own headings are not a package's to replace.
		system := &viewmodel.Notice{TitleKey: "plugin_callback_failed", Details: &protocol.NotificationDetails{Kind: "system"}, Presentation: reminder.Presentation}
		if shell.NoticeHeading(system, "en") != "" {
			t.Fatal("a package heading replaced a system heading")
		}
		plain := &viewmodel.Notice{Title: "Index", Message: "Rebuilt"}
		if title, body := noticeText(plain, "en"); title != "Index" || body != "Rebuilt" {
			t.Fatalf("plain notice: %q, %q", title, body)
		}
	})
	t.Run("a persistent notice stays in the list after its toast expires", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			root := fixture.root
			window := closeTestWindow(root)
			root.PresentNotice(&viewmodel.Notice{ID: "plain", Kind: viewmodel.NoticeInfo, Title: "Index", Message: "Rebuilt"})
			reminder := &viewmodel.Notice{ID: "reminder", Kind: viewmodel.NoticeInfo, Title: "Call the bank",
				Presentation: &protocol.NotificationPresentation{Heading: "Reminder", Persistent: true}}
			root.App.Data.Notices = []*viewmodel.Notice{reminder}
			root.PresentNotice(reminder)
			window.Frame()
			window.Advance(500 * time.Millisecond)
			if !window.HasText("Reminder") || !window.HasText("Call the bank") || !window.HasText("Rebuilt") {
				t.Fatalf("no notice toasts: %q", window.Texts())
			}
			window.Advance(6 * time.Second)
			window.Frame()
			if window.HasText("Call the bank") || window.HasText("Rebuilt") {
				t.Fatalf("the notice toasts did not expire: %q", window.Texts())
			}
			if len(root.App.Data.Notices) != 1 || root.App.Data.Notices[0] != reminder || !reminder.Presentation.Persistent {
				t.Fatal("expiring the toast changed the persistent notice")
			}
		})
	})
}
