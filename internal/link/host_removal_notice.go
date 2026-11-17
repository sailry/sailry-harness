package link

import (
	"context"
	"encoding/hex"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// HostRemovalNotice owns one already authenticated connection after local trust
// is removed. It can only notify the saved self-revocation or inspect its ID.
// It never reconnects, and belongs to the Link rather than the caller's view.
type HostRemovalNotice struct {
	link       *Link
	attempt    HostRemovalAttempt
	connection *iroh.Connection
	compressed bool
	open       func(context.Context) (contextStream, error)
	close      func()
	// The Link mutex protects the reservation and its ownership transition.
	detached bool
	started  bool
	released bool
	ctx      context.Context
	cancel   context.CancelFunc
}

// PrepareHostRemovalNotice persists the original request and reserves only an
// existing live connection. An offline host needs no notification or dial.
func (link *Link) PrepareHostRemovalNotice(ctx context.Context, host protocol.NodeID) (HostRemovalAttempt, *HostRemovalNotice, error) {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	attempt, err := link.PrepareHostRemoval(ctx, host)
	if err != nil || attempt.State == "completed" || attempt.State == "failed" {
		return attempt, nil, err
	}
	return link.reserveHostRemovalNotice(ctx, attempt)
}

// The caller holds trustUpdate, keeping the captured trust and connection from
// changing between persistence of the intent and its local removal reservation.
func (link *Link) reserveHostRemovalNotice(ctx context.Context, attempt HostRemovalAttempt) (HostRemovalAttempt, *HostRemovalNotice, error) {
	host := attempt.Request.Target
	var err error
	link.mu.Lock()
	defer link.mu.Unlock()
	if previous := link.removalNotices[host]; previous != nil {
		return attempt, nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "host removal is already pending"}
	}
	var connection *iroh.Connection
	var compressed bool
	if slot := link.outgoing[host]; slot != nil && link.trusted[host] && link.directions[host].AllowsOutbound() {
		slot.mu.Lock()
		if slot.connection != nil && slot.telemetry != nil && slot.ctx.Err() == nil {
			state, _, closed := slot.telemetry.snapshot()
			if !closed && state.Route != nil {
				connection, compressed = slot.connection, slot.info.Zstd
			}
		}
		slot.mu.Unlock()
	}
	if connection == nil {
		// A previous uncertain effect remains uncertain even when this explicit
		// removal cannot inspect it. A fresh request is known not to be sent.
		if attempt.State == "pending" {
			attempt.State = "failed"
			attempt.Fault = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "host removal notification skipped while offline"}
			err = link.RecordHostRemoval(context.WithoutCancel(ctx), host, attempt.Request.ID, attempt.State, attempt.Fault)
		}
		return attempt, nil, err
	}
	notice := &HostRemovalNotice{link: link, attempt: attempt, connection: connection, compressed: compressed}
	notice.open = func(ctx context.Context) (contextStream, error) { return connection.OpenBi(ctx) }
	notice.close = func() { _ = connection.Close(1, "peer removed") }
	if link.removalNotices == nil {
		link.removalNotices = map[protocol.NodeID]*HostRemovalNotice{}
	}
	link.removalNotices[host] = notice
	return attempt, notice, nil
}

// Start follows successful local removal. Its ten-second budget includes queue
// time, stream admission and completion; no request survives for a later retry.
func (notice *HostRemovalNotice) Start() error {
	link := notice.link
	link.mu.Lock()
	if notice.released || notice.started || link.ctx.Err() != nil || !notice.detached || link.directions[notice.attempt.Request.Target].AllowsOutbound() {
		link.mu.Unlock()
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "host removal notification is no longer active"}
	}
	notice.started = true
	notice.ctx, notice.cancel = link.timeoutContext(link.ctx, ioTimeout)
	link.mu.Unlock()
	context.AfterFunc(notice.ctx, notice.Close)
	if err := notice.record("unknown", nil); err != nil {
		notice.Close()
		return err
	}
	if err := link.spawn(notice.run); err != nil {
		_ = notice.record("unknown", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal notification could not start"})
		notice.Close()
		return err
	}
	return nil
}

// Close abandons an unstarted reservation after a failed local save, or releases
// the detached connection after notification, re-pairing or Link shutdown.
func (notice *HostRemovalNotice) Close() {
	link := notice.link
	link.mu.Lock()
	if notice.released {
		link.mu.Unlock()
		return
	}
	notice.released = true
	if link.removalNotices[notice.attempt.Request.Target] == notice {
		delete(link.removalNotices, notice.attempt.Request.Target)
	}
	detached, cancel := notice.detached, notice.cancel
	link.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	if detached {
		notice.close()
	}
}

func (link *Link) retainRemovalConnection(peer protocol.NodeID, connection *iroh.Connection) bool {
	link.mu.Lock()
	defer link.mu.Unlock()
	notice := link.removalNotices[peer]
	if notice == nil || notice.released || notice.connection != connection {
		return false
	}
	notice.detached = true
	return true
}

func (link *Link) cancelHostRemoval(peer protocol.NodeID) {
	link.mu.RLock()
	notice := link.removalNotices[peer]
	link.mu.RUnlock()
	if notice != nil {
		notice.Close()
	}
}

func (notice *HostRemovalNotice) record(state string, failure *protocol.Fault) error {
	return notice.link.RecordHostRemoval(context.WithoutCancel(notice.link.ctx), notice.attempt.Request.Target, notice.attempt.Request.ID, state, failure)
}

func (notice *HostRemovalNotice) run() {
	defer notice.Close()
	state, failure := "unknown", (*protocol.Fault)(nil)
	if err := notice.ctx.Err(); err != nil {
		failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal notification expired or was cancelled"}
	} else if notice.attempt.State == "unknown" {
		state, failure = notice.inspect()
	} else {
		_, err := notice.execute(notice.attempt.Request)
		if err == nil {
			state = "completed"
		} else {
			value := faultValue(err)
			failure = &value
			if value.Code != protocol.ErrorCodeOutcomeUnknown && notice.ctx.Err() == nil {
				state = "failed"
			}
		}
	}
	notice.Close()
	// A failed diagnostic write leaves the pre-dispatch unknown record intact.
	_ = notice.record(state, failure)
}

func (notice *HostRemovalNotice) execute(request protocol.Request) (protocol.Output, error) {
	if err := notice.ctx.Err(); err != nil {
		return protocol.Output{}, unknownCompletion()
	}
	stream, err := notice.open(notice.ctx)
	if err != nil {
		return protocol.Output{}, err
	}
	// dispatchStream normally observes durable completion under Link's lifetime.
	// Closing this stream also releases that observer when our budget expires.
	defer stream.Close()
	if notice.ctx.Err() != nil {
		return protocol.Output{}, unknownCompletion()
	}
	remote := notice.link.Remote(EndpointAddress{ID: notice.attempt.Request.Target})
	admission, err := remote.dispatchStream(notice.ctx, request, stream, notice.compressed)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case response, ok := <-admission.Completion:
		if !ok {
			return protocol.Output{}, unknownCompletion()
		}
		if response.Fault != nil {
			return protocol.Output{}, response.Fault
		}
		return response.Output, nil
	case <-notice.ctx.Done():
		return protocol.Output{}, unknownCompletion()
	}
}

func (notice *HostRemovalNotice) inspect() (string, *protocol.Fault) {
	body, err := json.Marshal(notice.attempt.Request)
	if err != nil {
		return "unknown", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal request could not be inspected"}
	}
	digest := blake3.Sum256(body)
	request := protocol.NewRequest(notice.attempt.Request.Target, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: notice.attempt.Request.ID, Digest: hex.EncodeToString(digest[:])}})
	output, err := notice.execute(request)
	if err != nil {
		failure := faultValue(err)
		return "unknown", &failure
	}
	var result protocol.OutputRequestOutcomePayload
	body, err = json.Marshal(output.Data)
	if err != nil || json.Unmarshal(body, &result) != nil || output.Kind != "request_outcome" || result.ID != notice.attempt.Request.ID {
		return "unknown", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal inspection returned an invalid outcome"}
	}
	switch result.Outcome.Kind {
	case "completed":
		var completed protocol.Result[protocol.Output, protocol.Fault]
		body, err = json.Marshal(result.Outcome.Data)
		if err == nil && json.Unmarshal(body, &completed) == nil {
			if completed.Ok != nil && completed.Err == nil {
				return "completed", nil
			}
			if completed.Err != nil && completed.Ok == nil {
				if completed.Err.Code == protocol.ErrorCodeOutcomeUnknown {
					return "unknown", completed.Err
				}
				return "failed", completed.Err
			}
		}
	case "not_admitted":
		return "failed", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "host removal request was not admitted"}
	}
	return "unknown", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal outcome remains unknown"}
}
