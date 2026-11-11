package node

import (
	"context"
	"database/sql"
	"errors"
	"net/http"
	"sync"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/backup"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// backupTransport is the Node's proxy-aware outbound transport with the
// storage read deadline. Proxy settings are resolved per request.
var backupTransport = sync.OnceValue(func() http.RoundTripper {
	transport := netproxy.Transport().Clone()
	transport.ResponseHeaderTimeout = 60 * time.Second
	return transport
})

func backupCommand(kind string) bool {
	switch kind {
	case "read_backup_settings", "save_backup_settings", "list_backups", "export_backup",
		"download_backup", "upload_backup", "restore_backup_file", "restore_backup":
		return true
	}
	return false
}

func (node *Node) dispatchBackup(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if request.Plugin != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "profile backups require a controller"}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if !accepted.Receipt.Durable {
		go func() {
			defer node.work.Done()
			defer close(completion)
			completion <- response(node.readBackup(ctx, caller, request))
		}()
		return accepted, nil
	}
	var previous *link.Response
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		if previous, err = admission.Existing(ctx, db, caller, request, body); err != nil || previous != nil {
			return err
		}
		if request.Command.Kind != "save_backup_settings" {
			if err := store.PrepareBackup(ctx, db, node.profile.Ownership.Path, request.Command); err != nil {
				return err
			}
		}
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	})
	if err != nil {
		node.work.Done()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		node.work.Done()
		return accepted, nil
	}
	go func() {
		defer node.work.Done()
		defer close(completion)
		completion <- node.executeBackup(caller, request)
	}()
	return accepted, nil
}

func (node *Node) readBackup(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	switch input := request.Command.Data.(type) {
	case *protocol.CommandDownloadBackupPayload:
		// The prepared archive outlives this request as a single-use transfer.
		value, err := node.downloadBackup(node.ctx, caller, request.ID, input.ExpectedRevision)
		return protocol.Output{Kind: "backup_download", Data: value}, err
	case *protocol.CommandUploadBackupPayload:
		value, err := node.transfers.UploadBackup(ctx, caller, input.Size, input.Revision)
		return protocol.Output{Kind: "backup_upload", Data: value}, err
	}
	switch request.Command.Kind {
	case "read_backup_settings":
		var value protocol.BackupSettings
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			value, err = store.ReadBackupSettings(ctx, db)
			return err
		})
		return protocol.Output{Kind: "backup_settings", Data: value}, err
	case "list_backups":
		plan, err := node.backupPlan(ctx, nil)
		if err != nil {
			return protocol.Output{}, err
		}
		value, err := backup.List(ctx, plan)
		return protocol.Output{Kind: "backups", Data: value}, err
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "backup read expected"}
}

func (node *Node) executeBackup(caller protocol.NodeID, request protocol.Request) link.Response {
	var output protocol.Output
	var operationErr error
	update, configuration := request.Command.Data.(*protocol.BackupUpdate)
	if !configuration {
		working := node.power.Hold()
		output, operationErr = node.changeBackup(node.ctx, caller, request)
		working()
	}
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		if configuration {
			var events []protocol.Event
			var settings protocol.BackupSettings
			if _, err := db.ExecContext(node.ctx, "SAVEPOINT sailry_backup_settings"); err != nil {
				return err
			}
			settings, events, operationErr = store.SaveBackupSettings(node.ctx, db, *update, node.dispatch.clock.Now().UnixMilli())
			if operationErr == nil && settings.Local != nil {
				// An export would refuse this directory; so does the save.
				operationErr = backup.LocalTarget(settings.Local.Directory, node.profile.Ownership.Path)
			}
			output = protocol.Output{Kind: "backup_settings", Data: settings}
			if operationErr != nil {
				events = nil
				if _, err := db.ExecContext(node.ctx, "ROLLBACK TO sailry_backup_settings"); err != nil {
					return err
				}
			}
			if _, err := db.ExecContext(node.ctx, "RELEASE sailry_backup_settings"); err != nil {
				return err
			}
			if _, err := store.RecordEvents(node.ctx, db, node.ID(), events); err != nil {
				return err
			}
		}
		if request.Command.Kind == "export_backup" {
			// The outcome is kept with the command's own result, so a
			// scheduled backup that failed with nobody connected is on the
			// Backup page when its user returns.
			if err := store.RecordBackupOutcome(node.ctx, db, backupOutcome(output, operationErr, node.dispatch.clock.Now().UnixMilli())); err != nil {
				return err
			}
		}
		return admission.Complete(node.ctx, db, caller, request.ID, response(output, operationErr))
	})
	if err != nil {
		if configuration {
			return node.unapplied(caller, request.ID, err)
		}
		// The operation ran outside the transaction, so its result is what is stored.
		return node.settle(caller, request.ID, response(output, operationErr))
	}
	if configuration && operationErr == nil {
		node.publish()
		node.wakeDispatch()
	}
	return response(output, operationErr)
}

// backupOutcome is the Backup page's record of one finished backup.
func backupOutcome(output protocol.Output, failure error, finished int64) protocol.BackupOutcome {
	outcome := protocol.BackupOutcome{FinishedMs: uint64(finished)}
	if failure == nil {
		if record, ok := output.Data.(protocol.BackupRecord); ok {
			outcome.Skipped = record.Skipped
		}
		return outcome
	}
	code, reason := protocol.ErrorCodeInternal, failure.Error()
	var fault *protocol.Fault
	if errors.As(failure, &fault) {
		code, reason = fault.Code, fault.Message
	}
	outcome.Error, outcome.Reason = &code, reason
	return outcome
}

func (node *Node) backupPlan(ctx context.Context, expected *uint64) (backup.Plan, error) {
	var plan backup.Plan
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		plan, err = store.BackupPlan(ctx, db, expected)
		return err
	})
	plan.Transport = backupTransport()
	return plan, err
}

// captureBackup pins one state of both databases, copies it with SQLite's
// online backup and archives the content that state lists. The exclusive
// leases last only until both read transactions exist: the Node keeps
// reading and writing while the copy, the capture and the upload run.
func (node *Node) captureBackup(ctx context.Context, revision uint64) (backup.Plan, *backup.Packed, error) {
	profile, storage := node.profile.Ownership.Path, node.profile.Store
	main, err := storage.Node.Exclusive(ctx)
	if err != nil {
		return backup.Plan{}, nil, err
	}
	knowledge, err := storage.Knowledge.Exclusive(ctx)
	if err != nil {
		_ = main.Close()
		return backup.Plan{}, nil, err
	}
	var pinned []*store.Pinned
	for _, exclusive := range []*store.Exclusive{main, knowledge} {
		if err == nil {
			var state *store.Pinned
			if state, err = exclusive.Pin(ctx); err == nil {
				pinned = append(pinned, state)
			}
		}
	}
	err = errors.Join(err, main.Close(), knowledge.Close())
	release := func() {
		for _, state := range pinned {
			_ = state.Close()
		}
	}
	defer release()
	if err != nil {
		return backup.Plan{}, nil, err
	}
	plan, roots, err := store.BackupSource(ctx, pinned[0].Connection(), node.ID(), profile, revision)
	if err != nil {
		return backup.Plan{}, nil, err
	}
	plan.Transport = backupTransport()
	passphrase, err := backup.Passphrase(plan.Auth.Passphrase)
	if err != nil {
		return backup.Plan{}, nil, err
	}
	snapshot, err := backup.NewSnapshot(profile, node.ID())
	if err != nil {
		return backup.Plan{}, nil, err
	}
	for index, name := range []string{"node.sqlite", "knowledge.sqlite"} {
		if err == nil {
			err = snapshot.Freeze(ctx, name, pinned[index].Copy)
		}
	}
	release()
	if err == nil {
		err = snapshot.Capture(ctx, roots)
	}
	var packed *backup.Packed
	if err == nil {
		packed, err = snapshot.Pack(ctx, passphrase)
	}
	if err != nil {
		_ = snapshot.Close()
		return backup.Plan{}, nil, err
	}
	return plan, packed, nil
}
