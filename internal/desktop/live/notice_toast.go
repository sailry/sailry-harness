package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PresentNotice shows a new notice as a toast of this window, whose Details
// action opens the notice's conversation, or the notification centre for a
// notice without one. A notice of a conversation the window already shows
// has no toast: the conversation tells it. The Notifier calls it on the UI
// thread between frames, for the active window only. In Demo mode the toast
// still shows, without Details.
func (r *Runtime) PresentNotice(notice *viewmodel.Notice) {
	root := r.localRuntime()
	if session, err := protocol.ParseSessionID(notice.Session); err == nil && root.seenConversation(session) {
		// conversationSeen holds the conversations the last frame built.
		return
	}
	root.enqueue(func(c *ui.Context) {
		title := notice.Title
		if notice.TitleKey != "" {
			title = i18n.T(c, notice.TitleKey)
		}
		kind := "info"
		switch notice.Kind {
		case viewmodel.NoticeCompleted:
			kind = "success"
		case viewmodel.NoticeFailed:
			kind = "error"
		case viewmodel.NoticeApproval:
			kind = "warning"
		}
		toast := ui.Toast{ID: "notice/" + notice.ID, Type: kind, Title: title, Description: notice.Message, Timeout: kit.ToastDuration(c)}
		// While Demo mode shows, the window's own conversations and notices
		// wait behind the demo's: the toast tells the notice and opens nothing.
		if !root.App.Settings.DemoMode {
			toast.Action = i18n.T(c, "notification_details")
			toast.OnAction = func() {
				if root.App.Settings.DemoMode {
					return
				}
				if notice.Session != "" {
					root.OpenNotice(notice.ID)
				} else {
					root.App.Navigate(shell.NotificationsPage)
				}
				root.wake()
			}
		}
		c.AddToast(toast)
	})
}
