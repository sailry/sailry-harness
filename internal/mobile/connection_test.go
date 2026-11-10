package mobile

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type noticeTransport struct {
	node protocol.NodeID
}

func (transport noticeTransport) Target() protocol.NodeID { return transport.node }
func (noticeTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, unavailable("test stream is unavailable")
}
func (noticeTransport) Dispatch(context.Context, protocol.Request) (link.Admission, error) {
	return link.Admission{}, unavailable("test command is unavailable")
}
func (noticeTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, unavailable("test subscription is unavailable")
}
func (noticeTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}

func TestMobileUnobservedConnectionPreservesInbox(t *testing.T) {
	t.Parallel()
	for _, method := range []string{"close", "watch", "watch_terminal"} {
		t.Run(method, func(t *testing.T) {
			node := protocol.NodeID{1}
			notice := client.Notice{
				ID: client.NoticeID{
					Node: node, Cursor: 1,
					Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NewNotificationID()},
				},
				Kind: "notification", NotificationKind: protocol.NotificationKindInfo,
			}
			controller := &controller{}
			controller.inbox.Merge(node, []client.Notice{notice})
			shared := client.New(noticeTransport{node: node})
			t.Cleanup(func() { _ = shared.Close() })
			owner := &connection{client: shared, controller: controller}
			if method == "close" {
				if err := owner.close(); err != nil {
					t.Fatal(err)
				}
			} else {
				argument := json.RawMessage("null")
				if method == "watch_terminal" {
					argument, _ = json.Marshal(protocol.NewTerminalID())
				}
				observer, err := shared.Watch(t.Context(), method, argument)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = observer.Close() })
				updates := &observation{observer: observer, connection: owner, notices: method == "watch"}
				if _, err := updates.invoke(t.Context(), nil, 0, "next", nil); err != nil {
					t.Fatal(err)
				}
			}
			retained := controller.inbox.Notices()
			if len(retained) != 1 || retained[0].ID != notice.ID || controller.inbox.Unread() != 1 {
				t.Fatal("an unobserved connection discarded retained Node notices")
			}
		})
	}
}
