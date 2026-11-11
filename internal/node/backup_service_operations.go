package node

import (
	"context"
	"encoding/hex"
	"errors"
	"io"
	"log/slog"
	"os"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/backup"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (node *Node) changeBackup(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	profile := node.profile.Ownership.Path
	switch input := request.Command.Data.(type) {
	case *protocol.CommandExportBackupPayload:
		plan, packed, err := node.captureBackup(ctx, input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, err
		}
		defer packed.Snapshot.Close()
		if plan.Local != nil && plan.Local.Enabled {
			if err := backup.LocalTarget(plan.Local.Directory, profile); err != nil {
				return protocol.Output{}, err
			}
		}
		record, err := backup.Export(ctx, plan, packed, request.ID.String())
		if err != nil {
			return protocol.Output{}, err
		}
		// The new backup is published; a retention failure does not undo it
		// and the next backup prunes again.
		if removed, err := backup.Prune(ctx, plan, node.ID()); err != nil {
			slog.Warn("Backup retention failed", "removed", removed, "error", err)
		}
		return protocol.Output{Kind: "backup", Data: record}, nil
	case *protocol.CommandRestoreBackupPayload:
		plan, err := node.backupPlan(ctx, &input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, err
		}
		if input.Passphrase != nil {
			plan.Auth.Passphrase = input.Passphrase
		}
		target, err := backup.TargetPath(input.Target, profile)
		if err != nil {
			return protocol.Output{}, err
		}
		temporary, err := core.Temporary(profile)
		if err != nil {
			return protocol.Output{}, err
		}
		record, sealed, err := backup.Fetch(ctx, plan, input.ID, input.Location, temporary)
		if err != nil {
			return protocol.Output{}, err
		}
		defer discard(sealed)
		passphrase, err := backup.Passphrase(plan.Auth.Passphrase)
		if err != nil {
			return protocol.Output{}, err
		}
		archive, err := backup.Open(ctx, sealed, passphrase)
		if err != nil {
			return protocol.Output{}, err
		}
		staged, err := backup.Stage(ctx, archive, temporary, &record, "", 0, "")
		if err != nil {
			return protocol.Output{}, err
		}
		return node.publishRestored(ctx, staged, target)
	case *protocol.CommandRestoreBackupFilePayload:
		plan, err := node.backupPlan(ctx, &input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, err
		}
		secret := input.Passphrase
		if secret == nil {
			secret = plan.Auth.Passphrase
		}
		passphrase, err := backup.Passphrase(secret)
		if err != nil {
			return protocol.Output{}, err
		}
		target, err := backup.TargetPath(input.Target, profile)
		if err != nil {
			return protocol.Output{}, err
		}
		temporary, err := core.Temporary(profile)
		if err != nil {
			return protocol.Output{}, err
		}
		uploaded, size, release, err := node.transfers.ClaimBackup(caller, input.Stream)
		if err != nil {
			return protocol.Output{}, err
		}
		// The uploaded archive stays claimed through restore validation.
		defer release()
		sum, err := backup.HashFile(ctx, uploaded, size)
		if err != nil {
			return protocol.Output{}, err
		}
		archive, err := backup.Open(ctx, uploaded, passphrase)
		if err != nil {
			return protocol.Output{}, err
		}
		staged, err := backup.Stage(ctx, archive, temporary, nil, request.ID.String(), size, sum)
		if err != nil {
			return protocol.Output{}, err
		}
		return node.publishRestored(ctx, staged, target)
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "backup operation expected"}
}

// publishRestored validates the staged profile and publishes it as the new
// target directory. The running profile is never written.
func (node *Node) publishRestored(ctx context.Context, staged *backup.Staged, target string) (protocol.Output, error) {
	defer staged.Close()
	identity, err := link.OpenIdentity(staged.Directory)
	if err != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "restored identity is unreadable"}
	}
	if identity.ID() != staged.Record.Node {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "restored identity differs from its database"}
	}
	retained, err := store.PrepareRestored(ctx, staged.Directory, staged.Record.Node, staged.Manifest.OriginalProfile, target)
	if err != nil {
		var failure *protocol.Fault
		if errors.As(err, &failure) || ctx.Err() != nil {
			return protocol.Output{}, err
		}
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "restored profile failed validation: " + err.Error()}
	}
	if err := backup.Finish(ctx, staged, target, retained); err != nil {
		return protocol.Output{}, err
	}
	return protocol.Output{Kind: "backup_restored", Data: protocol.OutputBackupRestoredPayload{Record: staged.Record, Target: target}}, nil
}

func (node *Node) downloadBackup(ctx context.Context, caller protocol.NodeID, id protocol.RequestID, revision uint64) (protocol.BackupDownload, error) {
	_, packed, err := node.captureBackup(ctx, revision)
	if err != nil {
		return protocol.BackupDownload{}, err
	}
	defer packed.Snapshot.Close()
	record := backup.NewRecord(packed, id.String())
	sealed, err := os.Open(packed.Path)
	if err != nil {
		return protocol.BackupDownload{}, err
	}
	hash := blake3.New(32, nil)
	_, err = io.Copy(hash, readerFunc(func(buffer []byte) (int, error) {
		if err := ctx.Err(); err != nil {
			return 0, err
		}
		return sealed.Read(buffer)
	}))
	_ = sealed.Close()
	if err != nil {
		return protocol.BackupDownload{}, err
	}
	return node.transfers.DownloadBackup(ctx, caller, packed.Path, record, hex.EncodeToString(hash.Sum(nil)))
}

type readerFunc func([]byte) (int, error)

func (read readerFunc) Read(buffer []byte) (int, error) { return read(buffer) }

func discard(file *os.File) {
	_ = file.Close()
	_ = os.Remove(file.Name())
}
