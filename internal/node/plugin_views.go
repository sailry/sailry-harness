package node

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) readPluginView(ctx context.Context, input protocol.CommandReadPluginViewPayload) (protocol.Output, error) {
	lifetime, cancel := context.WithCancel(ctx)
	defer cancel()
	stop := context.AfterFunc(node.ctx, cancel)
	defer stop()
	resolve := func() (protocol.PluginInfo, error) {
		var info protocol.PluginInfo
		err := node.profile.Store.Node.Read(lifetime, func(db *sql.Conn) error {
			var err error
			info, err = store.ResolvePluginViewInfo(lifetime, db, input.Package, input.Surface)
			return err
		})
		return info, err
	}
	info, err := resolve()
	if err != nil {
		return protocol.Output{}, err
	}
	read, bounded := context.WithTimeout(lifetime, 5*time.Second)
	bundle, err := node.plugins.manager.ReadDesktop(read, info, input.Surface)
	bounded()
	if errors.Is(err, context.DeadlineExceeded) && lifetime.Err() == nil {
		err = &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "plugin resource read deadline exceeded"}
	}
	if err != nil {
		return protocol.Output{}, err
	}
	if _, err = resolve(); err != nil {
		return protocol.Output{}, err
	}
	if node.ctx.Err() != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node is closed"}
	}
	return protocol.Output{Kind: "plugin_view", Data: bundle}, nil
}
