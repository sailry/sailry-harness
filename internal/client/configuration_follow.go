package client

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConfigurationFollow uses the selected host's authenticated personal namespace.
func (client *Client) ConfigurationFollow(ctx context.Context, command protocol.ConfigurationFollowCommand) (protocol.ConfigurationFollowStatus, error) {
	output, err := client.Do(ctx, protocol.Command{Kind: "configuration_follow", Data: &command})
	if err != nil {
		return protocol.ConfigurationFollowStatus{}, err
	}
	var status protocol.ConfigurationFollowStatus
	body, err := json.Marshal(output.Data)
	if err != nil {
		return status, err
	}
	err = json.Unmarshal(body, &status)
	return status, err
}
func (client *Client) ReadConfigurationFollow(ctx context.Context) (protocol.ConfigurationFollowStatus, error) {
	return client.ConfigurationFollow(ctx, protocol.ConfigurationFollowCommand{Action: "read"})
}
