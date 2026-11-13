package acceptance

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const ownershipLimit = 4 * 1024 * 1024

type ownershipSession struct {
	ID      protocol.SessionID `json:"id"`
	Removed bool               `json:"removed,omitempty"`
}

// Only identities and their digest are persisted. A Request contains captured
// configuration, which must never become part of an acceptance recording.
type ownershipRequest struct {
	ID      protocol.RequestID  `json:"id"`
	Kind    string              `json:"kind"`
	Digest  string              `json:"digest"`
	Session *protocol.SessionID `json:"session,omitempty"`
}

type ownershipLedger struct {
	Version  int                `json:"version"`
	Node     protocol.NodeID    `json:"node"`
	Sessions []ownershipSession `json:"sessions"`
	Requests []ownershipRequest `json:"requests"`
}

var ownershipMu sync.Mutex

// Own durably records an explicitly created acceptance session before any
// further work. It merges with previous runs, including removal tombstones.
func Own(directory string, node protocol.NodeID, session protocol.SessionID) error {
	if session == (protocol.SessionID{}) {
		return errors.New("acceptance ownership requires a session identity")
	}
	return updateOwnership(directory, node, func(ledger *ownershipLedger) error {
		ledger.own(session)
		return nil
	})
}

func (ledger *ownershipLedger) own(session protocol.SessionID) {
	if !slices.ContainsFunc(ledger.Sessions, func(value ownershipSession) bool { return value.ID == session }) {
		ledger.Sessions = append(ledger.Sessions, ownershipSession{ID: session})
	}
}

func (ledger ownershipLedger) owns(session protocol.SessionID) bool {
	return slices.ContainsFunc(ledger.Sessions, func(value ownershipSession) bool { return value.ID == session })
}

func updateOwnership(directory string, node protocol.NodeID, update func(*ownershipLedger) error) error {
	if node == (protocol.NodeID{}) {
		return errors.New("acceptance ownership requires a Node identity")
	}
	return withAcceptanceFiles(directory, func(absolute string) error {
		ledger, err := readOwnership(absolute, node)
		if err != nil {
			return err
		}
		if err := update(&ledger); err != nil {
			return err
		}
		data, err := json.MarshalIndent(ledger, "", "  ")
		if err != nil {
			return err
		}
		if len(data)+1 > ownershipLimit {
			return errors.New("acceptance ownership ledger exceeds its size limit")
		}
		return writeReportFile(filepath.Join(absolute, "ownership.json"), append(data, '\n'))
	})
}

func readOwnership(directory string, node protocol.NodeID) (ownershipLedger, error) {
	ledger := ownershipLedger{Version: 1, Node: node, Sessions: []ownershipSession{}, Requests: []ownershipRequest{}}
	data, err := readAcceptanceFile(filepath.Join(directory, "ownership.json"), ownershipLimit)
	if errors.Is(err, os.ErrNotExist) {
		err := mergeReportOwnership(directory, &ledger)
		return ledger, err
	}
	if err != nil {
		return ledger, err
	}
	if err := json.Unmarshal(data, &ledger); err != nil {
		return ledger, errors.New("acceptance ownership ledger is invalid; preserve it for inspection")
	}
	if ledger.Version != 1 || ledger.Node != node {
		return ledger, errors.New("acceptance ownership ledger belongs to another Node or version")
	}
	return ledger, nil
}

func loadOwnership(directory string, node protocol.NodeID) (ownershipLedger, error) {
	var ledger ownershipLedger
	err := withAcceptanceFiles(directory, func(absolute string) (err error) {
		ledger, err = readOwnership(absolute, node)
		return err
	})
	return ledger, err
}

// The lock spans processes as well as goroutines: the Host command and Desktop
// child can share a report directory. Its inode stays linked across updates.
func withAcceptanceFiles(directory string, use func(string) error) error {
	if directory == "" {
		return errors.New("acceptance report directory is required")
	}
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	ownershipMu.Lock()
	defer ownershipMu.Unlock()
	if err := core.PrivateDirectory(absolute); err != nil {
		return err
	}
	lock, err := lockAcceptanceFiles(filepath.Join(absolute, ".ownership.lock"))
	if err != nil {
		return err
	}
	defer lock.Close()
	return use(absolute)
}

func readAcceptanceFile(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > limit {
		return nil, errors.New("acceptance evidence must be a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err == nil && int64(len(data)) > limit {
		err = errors.New("acceptance evidence exceeds its size limit")
	}
	return data, err
}

func rememberRequest(directory string, request protocol.Request) error {
	body, err := json.Marshal(request)
	if err != nil {
		return err
	}
	digest := blake3.Sum256(body)
	// Decode just the routing identity, never retain the command's configuration.
	var target struct {
		Session *protocol.SessionID `json:"session"`
	}
	data, err := json.Marshal(request.Command.Data)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(data, &target); err != nil {
		return err
	}
	return updateOwnership(directory, request.Target, func(ledger *ownershipLedger) error {
		if target.Session != nil && !ledger.owns(*target.Session) {
			return errors.New("acceptance command targets a session outside its ownership ledger")
		}
		if !slices.ContainsFunc(ledger.Requests, func(value ownershipRequest) bool { return value.ID == request.ID }) {
			ledger.Requests = append(ledger.Requests, ownershipRequest{ID: request.ID, Kind: request.Command.Kind, Digest: hex.EncodeToString(digest[:]), Session: target.Session})
		}
		return nil
	})
}

func createdSessions(kind string, output protocol.Output) ([]protocol.SessionID, error) {
	switch kind {
	case "create_session", "fork_conversation":
		value, err := outputAs[protocol.Session](output, "session")
		if err != nil || value.ID == (protocol.SessionID{}) {
			return nil, errors.New("acceptance could not record the created session")
		}
		return []protocol.SessionID{value.ID}, nil
	case "replace_turn":
		value, err := outputAs[protocol.OutputTurnReplacedPayload](output, "turn_replaced")
		if err != nil {
			return nil, err
		}
		if value.History != nil && value.History.Backup != nil {
			return []protocol.SessionID{value.History.Backup.ID}, nil
		}
	case "rewind_conversation":
		value, err := outputAs[protocol.ConversationRewind](output, "rewound")
		if err != nil {
			return nil, err
		}
		if value.Backup != nil {
			return []protocol.SessionID{value.Backup.ID}, nil
		}
	}
	return nil, nil
}

func finishRequest(directory string, node protocol.NodeID, request ownershipRequest, output *protocol.Output) ([]protocol.SessionID, error) {
	var created []protocol.SessionID
	var err error
	if output != nil {
		created, err = createdSessions(request.Kind, *output)
		if err != nil {
			return nil, err
		}
	}
	err = updateOwnership(directory, node, func(ledger *ownershipLedger) error {
		for _, id := range created {
			ledger.own(id)
		}
		ledger.Requests = slices.DeleteFunc(ledger.Requests, func(value ownershipRequest) bool { return value.ID == request.ID })
		return nil
	})
	return created, err
}

// Execute journals a durable acceptance command before sending it. Unknown
// admission is inspected by its original identity, never replayed. Successful
// session creation and backup ownership are published atomically with settling
// that identity. This also covers a lost creation response before Own is called.
func Execute(ctx context.Context, controller *client.Client, directory string, command protocol.Command) (protocol.Output, error) {
	if controller == nil {
		return protocol.Output{}, errors.New("acceptance requires an existing Client")
	}
	return executeRecorded(ctx, controller, directory, controller.Prepare(command))
}

func executeRecorded(ctx context.Context, controller *client.Client, directory string, request protocol.Request) (protocol.Output, error) {
	if !request.Command.Durable() {
		return controller.Execute(ctx, request)
	}
	if err := rememberRequest(directory, request); err != nil {
		return protocol.Output{}, err
	}
	output, err := controller.Execute(ctx, request)
	if fault, ok := nodeFault(err); ok && fault.Code == protocol.ErrorCodeOutcomeUnknown {
		inspect, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		outcome, inspectErr := controller.Outcome(inspect, request)
		cancel()
		if inspectErr == nil {
			switch outcome.Kind {
			case "completed":
				completed, decodeErr := decode[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
				if decodeErr == nil && completed.Ok != nil {
					output, err = *completed.Ok, nil
				} else if decodeErr == nil && completed.Err != nil {
					err = completed.Err
				}
			case "not_admitted":
				err = &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Acceptance request was not admitted"}
			}
		}
	}
	identity := ownershipRequest{ID: request.ID, Kind: request.Command.Kind}
	if err == nil {
		if _, recordErr := finishRequest(directory, request.Target, identity, &output); recordErr != nil {
			return protocol.Output{}, unconfirmedRequest(identity, recordErr)
		}
		return output, nil
	}
	if fault, ok := nodeFault(err); ok && fault.Code != protocol.ErrorCodeOutcomeUnknown {
		if _, recordErr := finishRequest(directory, request.Target, identity, nil); recordErr != nil {
			return protocol.Output{}, unconfirmedRequest(identity, recordErr)
		}
		return protocol.Output{}, err
	}
	return protocol.Output{}, unconfirmedRequest(identity, err)
}

func unconfirmedRequest(request ownershipRequest, _ error) error {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: fmt.Sprintf("Acceptance %s request %s remains unconfirmed; inspect its recorded identity before retrying", request.Kind, request.ID.String())}
}

func recoverRequests(ctx context.Context, controller *client.Client, directory string) ([]protocol.SessionID, error) {
	ledger, err := loadOwnership(directory, controller.Target())
	if err != nil {
		return nil, err
	}
	var recovered []protocol.SessionID
	var problems []error
	for _, request := range ledger.Requests {
		output, err := controller.Do(ctx, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: request.Digest}})
		if err != nil {
			problems = append(problems, unconfirmedRequest(request, err))
			continue
		}
		value, err := outputAs[protocol.OutputRequestOutcomePayload](output, "request_outcome")
		if err != nil || value.ID != request.ID {
			problems = append(problems, unconfirmedRequest(request, err))
			continue
		}
		var result *protocol.Output
		switch value.Outcome.Kind {
		case "not_admitted":
		case "completed":
			completed, err := decode[protocol.Result[protocol.Output, protocol.Fault]](value.Outcome.Data)
			if err != nil || completed.Ok == nil && completed.Err == nil {
				problems = append(problems, unconfirmedRequest(request, err))
				continue
			}
			// A stored unknown-outcome fault is still unresolved. Keep the
			// original identity just as Client.Resolve does, so another cleanup
			// cannot treat a possibly admitted mutation as settled.
			if completed.Err != nil && completed.Err.Code == protocol.ErrorCodeOutcomeUnknown {
				problems = append(problems, unconfirmedRequest(request, completed.Err))
				continue
			}
			result = completed.Ok
		default:
			problems = append(problems, unconfirmedRequest(request, nil))
			continue
		}
		ids, err := finishRequest(directory, controller.Target(), request, result)
		if err != nil {
			problems = append(problems, unconfirmedRequest(request, err))
		} else {
			recovered = append(recovered, ids...)
		}
	}
	return recovered, errors.Join(problems...)
}

// OwnedSessions returns only explicitly recorded sessions that have not been
// confirmed removed. A runner uses it after a child exits before reporting its
// created sessions; result rows never establish cleanup ownership.
func OwnedSessions(directory string, node protocol.NodeID) ([]protocol.SessionID, error) {
	ledger, err := loadOwnership(directory, node)
	if err != nil {
		return nil, err
	}
	var sessions []protocol.SessionID
	for _, session := range ledger.Sessions {
		if !session.Removed {
			sessions = append(sessions, session.ID)
		}
	}
	return sessions, nil
}

// ImportReportOwnership merges old reports from fixed acceptance paths and
// explicit UUID fields before the caller replaces latest.json. Titles, global
// Node snapshots and filesystem scans never grant ownership.
func ImportReportOwnership(directory string, node protocol.NodeID) error {
	return updateOwnership(directory, node, func(ledger *ownershipLedger) error {
		return mergeReportOwnership(directory, ledger)
	})
}

func mergeReportOwnership(directory string, ledger *ownershipLedger) error {
	for _, name := range []string{"latest.json", "backend/latest.json", "e2e/latest.json"} {
		data, err := readAcceptanceFile(filepath.Join(directory, filepath.FromSlash(name)), ownershipLimit)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return err
		}
		var report Report
		if json.Unmarshal(data, &report) != nil || report.Version != 1 {
			return errors.New("acceptance report is invalid; preserve it for inspection")
		}
		if report.Node != (protocol.NodeID{}) && report.Node != ledger.Node {
			return errors.New("acceptance report belongs to another Node")
		}
		for _, result := range report.Cases {
			// Fixture and replay session IDs describe displayed data; they
			// never grant ownership of a session in the execution Node.
			if result.Group == "ui" || result.Group == "replay" || result.Group == "" && (report.Suite == "ui" || report.Suite == "replay") {
				continue
			}
			ids := append(slices.Clone(result.OwnedSessions), result.Session)
			for _, evidence := range result.Evidence {
				if evidence.Kind == "backup" || evidence.Kind == "fork" {
					fields := strings.Fields(evidence.Value)
					if len(fields) > 0 {
						ids = append(ids, fields[0])
					}
				}
			}
			for _, text := range ids {
				if text == "" {
					continue
				}
				id, err := protocol.ParseSessionID(text)
				if err != nil {
					return errors.New("acceptance report contains an invalid owned session identity")
				}
				ledger.own(id)
			}
		}
	}
	return nil
}
