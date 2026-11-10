package files

import (
	"context"
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// backupLimit is the profile archive limit of the backup service.
const backupLimit uint64 = 1024 * 1024 * 1024 * 1024

// DownloadBackup takes ownership of a sealed archive inside the profile's
// temporary directory and serves it once. Only the controller-facing backup
// service registers private archive files.
func (transfers *Transfers) DownloadBackup(ctx context.Context, caller protocol.NodeID, archive string, record protocol.BackupRecord, revision string) (protocol.BackupDownload, error) {
	output := protocol.BackupDownload{Record: record, Size: record.SizeBytes, Revision: revision}
	placeholder, root, name, err := transfers.temporary()
	if err != nil {
		return output, err
	}
	_ = placeholder.Close()
	entry := &transferEntry{download: true, backup: true, tempRoot: root, temporary: name, size: record.SizeBytes, revision: revision}
	// The archive replaces the reserved name, so the transfer owner removes it.
	if err = os.Rename(archive, filepath.Join(transfers.profile, "tmp", name)); err == nil {
		entry.file, err = root.Open(name)
	}
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		_ = root.Remove(name)
		cleanupTransfer(entry)
		return output, err
	}
	output.Stream, err = transfers.reserve(caller, "", entry)
	if err != nil {
		cleanupTransfer(entry)
	}
	return output, err
}

// UploadBackup reserves private staging for one sealed archive.
func (transfers *Transfers) UploadBackup(ctx context.Context, caller protocol.NodeID, size uint64, revision string) (protocol.BackupUpload, error) {
	if size == 0 || size > backupLimit {
		return protocol.BackupUpload{}, invalid("backup exceeds the upload limit")
	}
	if !validRevision(revision) {
		return protocol.BackupUpload{}, invalid("invalid backup content revision")
	}
	output := protocol.BackupUpload{Size: size, Revision: strings.ToLower(revision)}
	entry := &transferEntry{backup: true, size: size, revision: output.Revision}
	var err error
	entry.file, entry.tempRoot, entry.temporary, err = transfers.temporary()
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		cleanupTransfer(entry)
		return output, err
	}
	output.Stream, err = transfers.reserve(caller, "", entry)
	if err != nil {
		cleanupTransfer(entry)
	}
	return output, err
}

// ClaimBackup takes a completed archive upload out of the transfer table. The
// caller reads the file and then calls release, which removes the staging.
func (transfers *Transfers) ClaimBackup(caller protocol.NodeID, id protocol.StreamID) (file *os.File, size uint64, release func(), err error) {
	transfers.mu.Lock()
	entry := transfers.entries[id]
	if transfers.closed || entry == nil || entry.caller != caller || !entry.backup || entry.download ||
		entry.state != "uploaded" || !entry.expires.After(transfers.now()) {
		transfers.mu.Unlock()
		return nil, 0, nil, transferMissing()
	}
	delete(transfers.entries, id)
	transfers.mu.Unlock()
	return entry.file, entry.size, func() { cleanupTransfer(entry) }, nil
}
