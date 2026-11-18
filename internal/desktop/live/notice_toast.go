package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PresentNotice shows a notice in the active window. A notice of a
// conversation already on screen belongs to that conversation.
func (r *Runtime) PresentNotice(notice *viewmodel.Notice) {
	root := r.localRuntime()
	if session, err := protocol.ParseSessionID(notice.Session); err == nil && root.seenConversation(session) {
		// conversationSeen holds the conversations the last frame built.
		return
	}
	root.enqueue(func(c *ui.Context) {
		title, body := noticeText(notice, i18n.Match(c.Locale()))
		kind := "info"
		switch notice.Kind {
		case viewmodel.NoticeCompleted:
			kind = "success"
		case viewmodel.NoticeFailed:
			kind = "error"
		case viewmodel.NoticeApproval:
			kind = "warning"
		}
		c.AddToast(ui.Toast{ID: "notice/" + notice.ID, Type: kind, Title: title, Description: body, Timeout: kit.ToastDuration(c)})
	})
}
