package files

import (
	"context"
	"io"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (transfers *Transfers) UploadPlugin(ctx context.Context, caller protocol.NodeID, spec protocol.PluginUploadSpec) (protocol.PluginUpload, error) {
	output := protocol.PluginUpload{Spec: spec}
	if !validRevision(spec.Revision) || spec.Size == 0 || spec.Size > 128*1024*1024 {
		return output, invalid("invalid plugin upload content revision or size")
	}
	spec.Revision = strings.ToLower(spec.Revision)
	output.Spec = spec
	entry := &transferEntry{plugin: &spec, size: spec.Size, revision: spec.Revision}
	var err error
	entry.file, entry.tempRoot, entry.temporary, err = transfers.temporary()
	if err != nil {
		cleanupTransfer(entry)
		return output, err
	}
	if err := ctx.Err(); err != nil {
		cleanupTransfer(entry)
		return output, err
	}
	output.Stream, err = transfers.reserve(caller, "", entry)
	if err != nil {
		cleanupTransfer(entry)
	}
	return output, err
}

func (transfers *Transfers) StagePluginUpload(ctx context.Context, caller protocol.NodeID, data []byte) (protocol.PluginUpload, error) {
	digest := blake3.Sum256(data)
	spec := protocol.PluginUploadSpec{Size: uint64(len(data)), Revision: fmtRevision(digest[:])}
	output, err := transfers.UploadPlugin(ctx, caller, spec)
	if err != nil {
		return output, err
	}
	transfers.mu.Lock()
	entry := transfers.entries[output.Stream]
	if entry == nil {
		transfers.mu.Unlock()
		return output, transferMissing()
	}
	if entry.timer != nil {
		entry.timer()
	}
	entry.state = "inspecting"
	transfers.workers.Add(1)
	transfers.mu.Unlock()
	defer transfers.workers.Done()
	err = writeBytes(entry.file, data)
	if err == nil {
		err = entry.file.Sync()
	}
	transfers.mu.Lock()
	if err == nil && transfers.entries[output.Stream] == entry {
		entry.state, entry.expires = "uploaded", transfers.now().Add(openTimeout)
		transfers.expiryLocked(output.Stream, entry)
	} else {
		delete(transfers.entries, output.Stream)
	}
	transfers.mu.Unlock()
	if err != nil {
		cleanupTransfer(entry)
	}
	return output, err
}

func (transfers *Transfers) SetPluginFormat(caller protocol.NodeID, id protocol.StreamID, format protocol.PluginSkillsArchiveFormat) error {
	if _, err := format.MarshalJSON(); err != nil {
		return invalid("unsupported skill archive format")
	}
	transfers.mu.Lock()
	defer transfers.mu.Unlock()
	entry := transfers.entries[id]
	if entry == nil || entry.caller != caller || entry.plugin == nil || entry.state != "uploaded" || !entry.expires.After(transfers.now()) {
		return transferMissing()
	}
	entry.format = &format
	return nil
}

func (transfers *Transfers) PluginFormat(caller protocol.NodeID, id protocol.StreamID) (protocol.PluginSkillsArchiveFormat, error) {
	transfers.mu.Lock()
	defer transfers.mu.Unlock()
	entry := transfers.entries[id]
	if entry == nil || entry.caller != caller || entry.plugin == nil || entry.format == nil || !entry.expires.After(transfers.now()) {
		return "", transferMissing()
	}
	return *entry.format, nil
}

func (transfers *Transfers) UsePluginUpload(ctx context.Context, caller protocol.NodeID, id protocol.StreamID, consume bool, use func(io.ReaderAt, int64) error) error {
	transfers.mu.Lock()
	entry := transfers.entries[id]
	if transfers.closed || entry == nil || entry.caller != caller || entry.plugin == nil || entry.state != "uploaded" || !entry.expires.After(transfers.now()) {
		transfers.mu.Unlock()
		return transferMissing()
	}
	if entry.timer != nil {
		entry.timer()
	}
	entry.state = "inspecting"
	transfers.workers.Add(1)
	transfers.mu.Unlock()
	defer transfers.workers.Done()
	err := ctx.Err()
	if err == nil {
		err = use(entry.file, int64(entry.size))
	}
	transfers.mu.Lock()
	retained := transfers.entries[id] == entry
	remove := retained && consume && err == nil
	if remove {
		delete(transfers.entries, id)
	} else if retained {
		entry.state, entry.expires = "uploaded", transfers.now().Add(openTimeout)
		transfers.expiryLocked(id, entry)
	}
	transfers.mu.Unlock()
	if remove {
		cleanupTransfer(entry)
	}
	return err
}
