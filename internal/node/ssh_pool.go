package node

import (
	"errors"
	"sync"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/pkg/sftp"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

const (
	sshBrowsePoolMaxIdle = 8
	sshBrowsePoolIdleTTL = 2 * time.Minute
)

type sshBrowsePoolKey struct {
	profile  protocol.SshID
	revision uint64
}

type sshBrowseConn struct {
	closeOnce sync.Once
	conn      *cryptossh.Client
	sftp      *sftp.Client
	idleAt    time.Time
	used      uint64
}

func (entry *sshBrowseConn) close() error {
	var err error
	entry.closeOnce.Do(func() {
		var sftpErr, connErr error
		if entry.sftp != nil {
			sftpErr = entry.sftp.Close()
		}
		if entry.conn != nil {
			connErr = entry.conn.Close()
		}
		err = errors.Join(sftpErr, connErr)
	})
	return err
}

// sshBrowsePool holds bounded idle SSH+SFTP sessions keyed by (profile,
// revision) so repeated directory listings avoid redialing TCP/SSH/SFTP.
// Connections are closed on profile revision change, idle expiry, or Node stop.
type sshBrowsePool struct {
	clock   clockwork.Clock
	ttl     time.Duration
	maxIdle int
	mu      sync.Mutex
	closed  bool
	used    uint64
	idle    map[sshBrowsePoolKey]*sshBrowseConn
	timer   clockwork.Timer
}

func newSSHBrowsePool(clock clockwork.Clock) *sshBrowsePool {
	if clock == nil {
		clock = clockwork.NewRealClock()
	}
	return &sshBrowsePool{
		clock:   clock,
		ttl:     sshBrowsePoolIdleTTL,
		maxIdle: sshBrowsePoolMaxIdle,
		idle:    map[sshBrowsePoolKey]*sshBrowseConn{},
	}
}

func (pool *sshBrowsePool) acquire(profile protocol.SshID, revision uint64) *sshBrowseConn {
	if pool == nil {
		return nil
	}
	now := pool.clock.Now()
	var expired []*sshBrowseConn
	pool.mu.Lock()
	if pool.closed {
		pool.mu.Unlock()
		return nil
	}
	expired = pool.evictExpiredLocked(now)
	key := sshBrowsePoolKey{profile: profile, revision: revision}
	entry := pool.idle[key]
	if entry != nil {
		delete(pool.idle, key)
	}
	pool.rescheduleLocked(now)
	pool.mu.Unlock()
	for _, old := range expired {
		_ = old.close()
	}
	return entry
}

func (pool *sshBrowsePool) release(profile protocol.SshID, revision uint64, entry *sshBrowseConn) {
	if entry == nil {
		return
	}
	if pool == nil {
		_ = entry.close()
		return
	}
	now := pool.clock.Now()
	var toClose []*sshBrowseConn
	pool.mu.Lock()
	if pool.closed {
		pool.mu.Unlock()
		_ = entry.close()
		return
	}
	toClose = append(toClose, pool.evictExpiredLocked(now)...)
	for key, existing := range pool.idle {
		if key.profile == profile && key.revision != revision {
			delete(pool.idle, key)
			toClose = append(toClose, existing)
		}
	}
	key := sshBrowsePoolKey{profile: profile, revision: revision}
	if existing := pool.idle[key]; existing != nil {
		delete(pool.idle, key)
		toClose = append(toClose, existing)
	}
	for len(pool.idle) >= pool.maxIdle {
		var oldestKey sshBrowsePoolKey
		var oldest *sshBrowseConn
		for k, v := range pool.idle {
			if oldest == nil || v.used < oldest.used {
				oldestKey, oldest = k, v
			}
		}
		if oldest == nil {
			break
		}
		delete(pool.idle, oldestKey)
		toClose = append(toClose, oldest)
	}
	pool.used++
	entry.idleAt = now
	entry.used = pool.used
	pool.idle[key] = entry
	pool.rescheduleLocked(now)
	pool.mu.Unlock()
	for _, old := range toClose {
		_ = old.close()
	}
}

func (pool *sshBrowsePool) invalidate(profile protocol.SshID) {
	if pool == nil {
		return
	}
	var toClose []*sshBrowseConn
	pool.mu.Lock()
	for key, entry := range pool.idle {
		if key.profile == profile {
			delete(pool.idle, key)
			toClose = append(toClose, entry)
		}
	}
	pool.rescheduleLocked(pool.clock.Now())
	pool.mu.Unlock()
	for _, old := range toClose {
		_ = old.close()
	}
}

func (pool *sshBrowsePool) close() error {
	if pool == nil {
		return nil
	}
	var toClose []*sshBrowseConn
	pool.mu.Lock()
	if pool.closed {
		pool.mu.Unlock()
		return nil
	}
	pool.closed = true
	if pool.timer != nil {
		pool.timer.Stop()
		pool.timer = nil
	}
	for key, entry := range pool.idle {
		delete(pool.idle, key)
		toClose = append(toClose, entry)
	}
	pool.mu.Unlock()
	var errs []error
	for _, entry := range toClose {
		errs = append(errs, entry.close())
	}
	return errors.Join(errs...)
}

func (pool *sshBrowsePool) evictExpiredLocked(now time.Time) []*sshBrowseConn {
	if pool.ttl <= 0 {
		return nil
	}
	var expired []*sshBrowseConn
	for key, entry := range pool.idle {
		if now.Sub(entry.idleAt) >= pool.ttl {
			delete(pool.idle, key)
			expired = append(expired, entry)
		}
	}
	return expired
}

func (pool *sshBrowsePool) rescheduleLocked(now time.Time) {
	if pool.timer != nil {
		pool.timer.Stop()
		pool.timer = nil
	}
	if pool.closed || pool.ttl <= 0 || len(pool.idle) == 0 {
		return
	}
	var earliest time.Time
	for _, entry := range pool.idle {
		if earliest.IsZero() || entry.idleAt.Before(earliest) {
			earliest = entry.idleAt
		}
	}
	delay := pool.ttl - now.Sub(earliest)
	if delay < 0 {
		delay = 0
	}
	pool.timer = pool.clock.AfterFunc(delay, pool.reap)
}

func (pool *sshBrowsePool) reap() {
	now := pool.clock.Now()
	pool.mu.Lock()
	if pool.closed {
		pool.mu.Unlock()
		return
	}
	expired := pool.evictExpiredLocked(now)
	pool.rescheduleLocked(now)
	pool.mu.Unlock()
	for _, entry := range expired {
		_ = entry.close()
	}
}

func sshConfigurationProfileID(command protocol.Command) (protocol.SshID, bool) {
	switch data := command.Data.(type) {
	case *protocol.CommandSaveSshPayload:
		return data.Profile.ID, true
	case protocol.CommandSaveSshPayload:
		return data.Profile.ID, true
	case *protocol.CommandTrustSshPayload:
		return data.Profile, true
	case protocol.CommandTrustSshPayload:
		return data.Profile, true
	case *protocol.CommandRemoveSshPayload:
		return data.Profile, true
	case protocol.CommandRemoveSshPayload:
		return data.Profile, true
	default:
		return protocol.SshID{}, false
	}
}
