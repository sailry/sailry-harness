package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConfigurationCommandDelivery retains the original request until its outcome
// is known. Passing nil recovers an interrupted delivery without creating one.
func ConfigurationCommandDelivery(ctx context.Context, db *sql.Conn, host protocol.NodeID, category string, command *protocol.ConfigurationFollowCommand) (*protocol.Request, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	previous, err := readBody[protocol.Request](ctx, db, "SELECT request FROM configuration_follow_deliveries WHERE user=?1 AND host=?2 AND category=?3", user.ID.String(), host[:], category)
	if err != nil || previous != nil || command == nil {
		return previous, err
	}
	request := protocol.NewRequest(host, protocol.Command{Kind: "configuration_follow", Data: command})
	body, err := encode(request)
	if err != nil {
		return nil, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_deliveries(user,host,category,request) VALUES(?1,?2,?3,?4)", user.ID.String(), host[:], category, body)
	return &request, err
}
