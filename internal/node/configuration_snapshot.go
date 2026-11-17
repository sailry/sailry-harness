package node

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// writeConfigurationSnapshot encodes one record at a time. The persisted
// delivery is the source of truth; the temporary file only bounds transmission
// memory and can be rebuilt exactly after a restart.
func writeConfigurationSnapshot(writer io.Writer, manifest protocol.ConfigurationManifest) error {
	header := struct {
		Source   protocol.NodeID `json:"source"`
		Category string          `json:"category"`
		Revision uint64          `json:"revision"`
	}{manifest.Source, manifest.Category, manifest.Revision}
	body, err := json.Marshal(header)
	if err != nil {
		return err
	}
	if _, err := writer.Write(body[:len(body)-1]); err != nil {
		return err
	}
	if _, err := io.WriteString(writer, `,"records":[`); err != nil {
		return err
	}
	for index, record := range manifest.Records {
		if index != 0 {
			if _, err := io.WriteString(writer, ","); err != nil {
				return err
			}
		}
		body, err := json.Marshal(record)
		if err != nil {
			return err
		}
		if _, err := writer.Write(body); err != nil {
			return err
		}
	}
	_, err = io.WriteString(writer, "]}")
	return err
}

func (node *Node) deliverConfigurationSnapshot(ctx context.Context, remote link.Transport, request protocol.Request) error {
	input, valid := dispatchPayload[protocol.ConfigurationFollowCommand](request.Command.Data)
	if !valid || (input.Manifest == nil) == (input.KVReceipt == nil) || request.Target != remote.Target() {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration delivery is invalid"}
	}
	directory := filepath.Join(node.profile.Ownership.Path, "tmp")
	if err := os.MkdirAll(directory, 0700); err != nil {
		return err
	}
	file, err := os.CreateTemp(directory, "configuration-snapshot-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	defer file.Close()
	digest := blake3.New(32, nil)
	writer := io.MultiWriter(file, digest)
	var source protocol.NodeID
	var category string
	var revision uint64
	if input.Manifest != nil {
		source, category, revision = input.Manifest.Source, input.Manifest.Category, input.Manifest.Revision
		err = writeConfigurationSnapshot(writer, *input.Manifest)
	} else {
		source, category, revision = node.ID(), protocol.PersonalKVAcknowledgementCategory, input.KVReceipt.Sequence
		err = json.NewEncoder(writer).Encode(input.KVReceipt)
	}
	if err != nil {
		return err
	}
	size, err := file.Seek(0, io.SeekCurrent)
	if err != nil {
		return err
	}
	if size <= 0 || uint64(size) > protocol.MaxConfigurationSnapshotBytes {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration snapshot exceeds its limit"}
	}
	page := protocol.ConfigurationSnapshotPage{
		Root: request.ID, Source: source, Category: category,
		Revision: revision, Bytes: uint64(size), Digest: hex.EncodeToString(digest.Sum(nil)),
	}
	output, err := configurationRemote(ctx, remote, protocol.NewRequest(request.Target, protocol.Command{
		Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "snapshot_state", Page: &page},
	}))
	if err != nil {
		return err
	}
	count := (page.Bytes + protocol.ConfigurationPageBytes - 1) / protocol.ConfigurationPageBytes
	var progress protocol.ConfigurationSnapshotProgress
	if err := decodeConfigurationOutput(output, &progress); err != nil {
		return err
	}
	if progress.Root != page.Root || progress.NextIndex > count || progress.Complete && progress.NextIndex != count ||
		!progress.Complete && progress.NextIndex == count {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration snapshot progress is invalid"}
	}
	if progress.Complete {
		return nil
	}
	if _, err := file.Seek(int64(progress.NextIndex*protocol.ConfigurationPageBytes), io.SeekStart); err != nil {
		return err
	}
	buffer := make([]byte, protocol.ConfigurationPageBytes)
	defer clear(buffer)
	for index := progress.NextIndex; index < count; index++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		length := min(uint64(len(buffer)), page.Bytes-index*protocol.ConfigurationPageBytes)
		if _, err := io.ReadFull(file, buffer[:length]); err != nil {
			return err
		}
		page.Index, page.Data = index, buffer[:length]
		part := protocol.Request{
			Version: request.Version, Target: request.Target,
			ID:      protocol.ConfigurationPageRequestID(request.ID, index),
			Command: protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply_page", Page: &page}},
		}
		output, err := configurationRemote(ctx, remote, part)
		if err != nil {
			return err
		}
		if err := decodeConfigurationOutput(output, &progress); err != nil {
			return err
		}
		if progress.Root != page.Root || progress.NextIndex != index+1 || progress.Complete != (index+1 == count) {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration snapshot page receipt is invalid"}
		}
	}
	return nil
}

func (node *Node) applyConfigurationSnapshotPage(ctx context.Context, caller protocol.NodeID, request protocol.Request, page *protocol.ConfigurationSnapshotPage) (protocol.Output, error) {
	if page == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration snapshot page is required"}
	}
	var progress protocol.ConfigurationSnapshotProgress
	var changes []protocol.Event
	publication := node.agent.LockPublication()
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := node.authorizeCommandDB(ctx, db, caller, request); err != nil {
			return err
		}
		var err error
		progress, changes, err = store.StageConfigurationSnapshot(ctx, db, node.ID(), caller, request.ID, *page)
		if err != nil || len(changes) == 0 {
			return err
		}
		changes, err = node.followKnowledgeStorage(ctx, db, request, changes)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		return err
	})
	if err == nil && len(changes) != 0 {
		node.conversationEvents(changes)
		node.systemEvents(changes)
	}
	publication()
	if err == nil && len(changes) != 0 {
		node.pluginCommitted(changes, ctx, false)
		node.refreshKnowledgeStorage(request.Command, changes)
	}
	return protocol.Output{Kind: "configuration_snapshot", Data: progress}, err
}
