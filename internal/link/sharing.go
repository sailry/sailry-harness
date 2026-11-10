package link

import (
	"context"
	"io"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type ShareState struct {
	Kind        string          `json:"kind"`
	Code        string          `json:"code,omitempty"`
	ExpiresAtMS uint64          `json:"expires_at_ms,omitempty"`
	Fault       *protocol.Fault `json:"fault,omitempty"`
}

type ShareSession struct {
	link    *Link
	relay   *pairingRelay
	ctx     context.Context
	cancel  context.CancelFunc
	done    chan struct{}
	next    sync.Mutex
	mu      sync.Mutex
	state   ShareState
	version uint64
	seen    uint64
	changed chan struct{}
	err     error
}

func (link *Link) Share(ctx context.Context, origin string) (*ShareSession, error) {
	relay, err := newPairingRelay(origin, link.now)
	if err != nil {
		return nil, err
	}
	lifetime, cancel := link.operation(ctx, false)
	session := &ShareSession{
		link: link, relay: relay, ctx: lifetime, cancel: cancel, done: make(chan struct{}),
		state: ShareState{Kind: "preparing"}, version: 1, changed: make(chan struct{}),
	}
	go session.drive()
	return session, nil
}

func (session *ShareSession) Next(ctx context.Context) (ShareState, error) {
	session.next.Lock()
	defer session.next.Unlock()
	for {
		finished := false
		select {
		case <-session.done:
			finished = true
		default:
		}
		session.mu.Lock()
		if session.seen != session.version {
			session.seen = session.version
			state := session.state
			if state.Fault != nil {
				fault := *state.Fault
				state.Fault = &fault
			}
			session.mu.Unlock()
			return state, nil
		}
		changed, err := session.changed, session.err
		session.mu.Unlock()
		if finished {
			if err != nil {
				return ShareState{}, err
			}
			return ShareState{}, io.EOF
		}
		select {
		case <-ctx.Done():
			return ShareState{}, ctx.Err()
		case <-changed:
		case <-session.done:
		}
	}
}

func (session *ShareSession) Close() error {
	session.cancel()
	<-session.done
	return nil
}

func (session *ShareSession) publish(state ShareState) {
	session.mu.Lock()
	session.state = state
	session.version++
	close(session.changed)
	session.changed = make(chan struct{})
	session.mu.Unlock()
}

func (session *ShareSession) drive() {
	defer close(session.done)
	defer session.cancel()
	defer session.relay.client.CloseIdleConnections()
	for {
		if session.ctx.Err() != nil {
			session.publish(ShareState{Kind: "closed"})
			return
		}
		session.publish(ShareState{Kind: "preparing"})
		ended, err := session.shareInvitation()
		if err != nil {
			session.mu.Lock()
			session.err = err
			session.mu.Unlock()
			return
		}
		if ended {
			return
		}
	}
}

func (session *ShareSession) shareInvitation() (bool, error) {
	link := session.link
	invitation, err := link.Invite(session.ctx)
	if err != nil {
		return true, err
	}
	defer invitation.Close()
	request, err := newPairingRequestID(link.now())
	if err != nil {
		return true, err
	}
	deadline, expire := link.timeoutContext(session.ctx, max(0, invitation.deadline.Sub(link.now())))
	defer expire()
	var code pairingCode
	for {
		if deadline.Err() != nil || !link.now().Before(invitation.deadline) {
			if session.ctx.Err() != nil {
				session.publish(ShareState{Kind: "closed"})
				return true, nil
			}
			return false, nil
		}
		code, err = session.relay.publish(deadline, invitation, request)
		if err == nil {
			break
		}
		fault := faultValue(err)
		session.publish(ShareState{Kind: "retrying", Fault: &fault})
		if fault.Code != protocol.ErrorCodeUnavailable && fault.Code != protocol.ErrorCodeBusy {
			return true, err
		}
		retry, release := link.timeoutContext(deadline, 15*time.Second)
		<-retry.Done()
		release()
	}
	session.publish(ShareState{Kind: "ready", Code: code.code, ExpiresAtMS: code.expiresAtMS})
	paired := false
	select {
	case <-session.ctx.Done():
	case <-deadline.Done():
	case <-invitation.offer.stop:
	case <-invitation.offer.paired:
		paired = session.ctx.Err() == nil
	}
	_ = invitation.Close()
	ended := paired || session.ctx.Err() != nil || link.now().Before(invitation.deadline)
	state := ShareState{Kind: "preparing"}
	if paired {
		state.Kind = "paired"
	} else if ended {
		state.Kind = "closed"
	}
	session.publish(state)
	cleanup, cancel := link.timeoutContext(context.WithoutCancel(session.ctx), 2*time.Second)
	_ = session.relay.cancel(cleanup, code)
	cancel()
	return ended, nil
}
