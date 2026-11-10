package client

import (
	"bytes"
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type capturedTransferBuffer struct{ bytes.Buffer }

func (b *capturedTransferBuffer) Close() error { return nil }
func (c *Client) AttachmentBytes(ctx context.Context, attachment protocol.Attachment) ([]byte, error) {
	output, err := c.Do(ctx, protocol.Command{Kind: "download_attachment", Data: protocol.CommandDownloadAttachmentPayload{Worktree: attachment.Spec.Worktree, Attachment: attachment.ID}})
	if err != nil {
		return nil, err
	}
	return c.receiveAttachmentBytes(ctx, output, attachment)
}
func (c *Client) ImageBytes(ctx context.Context, session protocol.SessionID, image protocol.ConversationImage) ([]byte, error) {
	output, err := c.Do(ctx, protocol.Command{Kind: "download_image", Data: protocol.CommandDownloadImagePayload{Session: session, Image: image}})
	if err != nil {
		return nil, err
	}
	return c.receiveAttachmentBytes(ctx, output, image.Attachment)
}
func (c *Client) receiveAttachmentBytes(ctx context.Context, output protocol.Output, expected protocol.Attachment) ([]byte, error) {
	value, err := payload[protocol.AttachmentDownload](output.Data)
	if err != nil || output.Kind != "attachment_download" || value.Attachment != expected {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "attachment changed before download")
	}
	var destination capturedTransferBuffer
	if err := c.Download(ctx, value.Stream, expected.Spec.Size, expected.Spec.Revision, &destination, nil); err != nil {
		return nil, err
	}
	return destination.Bytes(), nil
}
