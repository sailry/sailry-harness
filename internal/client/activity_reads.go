package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (client *Client) ReadActivityCatalog(ctx context.Context, scope protocol.PluginContext) (protocol.ActivityCatalog, error) {
	request := client.Prepare(protocol.Command{Kind: "read_activity_catalog"})
	captured := copyValue(scope)
	request.Plugin = &captured
	output, err := client.Execute(ctx, request)
	if err != nil {
		return protocol.ActivityCatalog{}, err
	}
	catalog, err := payload[protocol.ActivityCatalog](output.Data)
	if output.Kind != "activity_catalog" || err != nil {
		return protocol.ActivityCatalog{}, fault(protocol.ErrorCodeInternal, "activity catalog response expected")
	}
	return catalog, nil
}

func (client *Client) ReadActivityPreviews(ctx context.Context, scope protocol.PluginContext, sessions []protocol.SessionID) ([]protocol.ActivityPreview, error) {
	if len(sessions) > 8 {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "activity preview batch exceeds the limit")
	}
	request := client.Prepare(protocol.Command{Kind: "read_activity", Data: protocol.CommandReadActivityPayload{Sessions: sessions}})
	captured := copyValue(scope)
	request.Plugin = &captured
	output, err := client.Execute(ctx, request)
	if err != nil {
		return nil, err
	}
	previews, err := payload[[]protocol.ActivityPreview](output.Data)
	if output.Kind != "activity" || err != nil {
		return nil, fault(protocol.ErrorCodeInternal, "activity previews response expected")
	}
	return previews, nil
}
