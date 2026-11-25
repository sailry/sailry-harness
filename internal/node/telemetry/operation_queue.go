package telemetry

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"syscall"
	"time"

	"github.com/getsentry/sentry-go"
	"github.com/sailry/sailry-harness/internal/node/logging"
)

const (
	operationSlots    = 32
	operationBytes    = 2048 // per slot; at most 64 KiB per profile
	operationTTL      = 24 * time.Hour
	operationRetries  = 3
	operationInterval = 5 * time.Minute
)

// Only a compact classification is durable, not the Sentry envelope, stack,
// breadcrumbs or attachments. Event and fingerprint are the reporting IDs,
// not Node request IDs. Retries keep both so Sentry can deduplicate them.
type pendingOperation struct {
	Scope       string         `json:"scope"`
	Event       sentry.EventID `json:"event"`
	Fingerprint string         `json:"fingerprint"`
	At          time.Time      `json:"at"`
	Next        time.Time      `json:"next"`
	Fields      logging.Fields `json:"fields"`
	Retries     int            `json:"retries"`
}

type operationQueue struct {
	ctx     context.Context
	cancel  context.CancelFunc
	done    chan struct{}
	actions chan pendingOperation
	scope   string
}

func newOperationQueue(c *Control) *operationQueue {
	ctx, cancel := context.WithCancel(context.Background())
	scope := c.installation + "\x00" + c.dsn + "\x00" + c.application + "\x00" + c.release
	// Preserve existing Harness retries without relabeling them as another product.
	if c.product != "harness" {
		scope += "\x00" + c.product
	}
	binding := sha256.Sum256([]byte(scope))
	return &operationQueue{ctx: ctx, cancel: cancel, done: make(chan struct{}), actions: make(chan pendingOperation, operationSlots), scope: hex.EncodeToString(binding[:])}
}

func (q *operationQueue) offer(event *sentry.Event, delay time.Duration) {
	item := pendingOperation{Scope: q.scope, Event: event.EventID, Fingerprint: event.Fingerprint[0], At: event.Timestamp, Next: time.Now().UTC().Add(delay), Fields: logging.Fields(event.Contexts["sailry"])}
	select {
	case q.actions <- item:
	default:
	}
}

func retryNetwork(err error) bool {
	var network net.Error
	var dns *net.DNSError
	return errors.As(err, &network) && network.Timeout() || errors.As(err, &dns) && dns.IsTemporary || errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) ||
		errors.Is(err, syscall.ECONNRESET) || errors.Is(err, syscall.ECONNREFUSED) || errors.Is(err, syscall.ECONNABORTED) ||
		errors.Is(err, syscall.ENETUNREACH) || errors.Is(err, syscall.EHOSTUNREACH) || errors.Is(err, syscall.EPIPE)
}

func retryHTTP(response *http.Response) time.Duration {
	switch response.StatusCode {
	case 408, 429, 500, 502, 503, 504:
	default:
		return 0
	}
	// Do not submit through an explicit Sentry quota. A standard server
	// cooldown can extend the wait, but not the retention period.
	if response.Header.Get("X-Sentry-Rate-Limits") != "" {
		return 0
	}
	if after := response.Header.Get("Retry-After"); after != "" {
		if seconds, err := strconv.ParseUint(after, 10, 64); err == nil {
			if seconds >= uint64(operationTTL/time.Second) {
				return 0
			}
			return max(operationInterval, time.Duration(seconds)*time.Second)
		}
		at, err := http.ParseTime(after)
		if err != nil || time.Until(at) >= operationTTL {
			return 0
		}
		return max(operationInterval, time.Until(at))
	}
	return operationInterval
}

func operationName(slot int) string { return fmt.Sprintf("operation-report-%02d.json", slot) }

// Pin the owned private directory, rejecting a replacement symlink before
// touching its contents. All opens and unlinks below use fixed basenames.
func operationRoot(directory string) *os.Root {
	// In production the parent is the canonical, exclusively owned Node
	// profile. Never enter an alias substituted for that directory.
	parentPath, name := filepath.Dir(directory), filepath.Base(directory)
	owner, err := os.Lstat(parentPath)
	if err != nil || !owner.IsDir() {
		return nil
	}
	parent, err := os.OpenRoot(parentPath)
	if err != nil {
		return nil
	}
	defer parent.Close()
	if opened, err := parent.Stat("."); err != nil || !os.SameFile(owner, opened) {
		return nil
	}
	info, err := parent.Lstat(name)
	if err != nil || !info.IsDir() {
		return nil
	}
	root, err := parent.OpenRoot(name)
	if err != nil {
		return nil
	}
	if opened, err := root.Stat("."); err != nil || !os.SameFile(info, opened) {
		root.Close()
		return nil
	}
	return root
}

func purgeOperations(directory string) {
	if root := operationRoot(directory); root != nil {
		defer root.Close()
		for slot := range operationSlots {
			_ = root.Remove(operationName(slot))
		}
	}
}

func (item pendingOperation) valid(scope string, now time.Time) bool {
	hexID := func(id string, length int) bool {
		if len(id) != length {
			return false
		}
		_, err := hex.DecodeString(id)
		return err == nil
	}
	age := now.Sub(item.At)
	return item.Scope == scope && age >= 0 && age < operationTTL && !item.Next.Before(item.At) && item.Next.Before(item.At.Add(operationTTL)) && item.Retries >= 0 && item.Retries < operationRetries &&
		hexID(string(item.Event), 32) && hexID(item.Fingerprint, 64) && item.Fields["phase"] == logging.Failed
}

func readOperation(root *os.Root, name, scope string) *pendingOperation {
	info, err := root.Lstat(name)
	if err != nil || !info.Mode().IsRegular() || info.Size() > operationBytes {
		return nil
	}
	file, err := root.OpenFile(name, operationReadFlags, 0)
	if err != nil {
		return nil
	}
	defer file.Close()
	if opened, err := file.Stat(); err != nil || !os.SameFile(info, opened) {
		return nil
	}
	body, err := io.ReadAll(io.LimitReader(file, operationBytes+1))
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	var item pendingOperation
	if err != nil || len(body) > operationBytes || !json.Valid(body) || decoder.Decode(&item) != nil || !item.valid(scope, time.Now()) {
		return nil
	}
	for key, value := range item.Fields {
		if number, ok := value.(json.Number); ok {
			if signed, err := number.Int64(); err == nil {
				item.Fields[key] = signed
			} else if unsigned, err := strconv.ParseUint(number.String(), 10, 64); err == nil {
				item.Fields[key] = unsigned
			}
		}
	}
	item.Fields = logging.Remote(item.Fields)
	return &item
}

func writeOperation(root *os.Root, name string, item pendingOperation) bool {
	body, err := json.Marshal(item)
	if err != nil || len(body) > operationBytes {
		return false
	}
	file, err := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return false
	}
	_, err = file.Write(body)
	if closeErr := file.Close(); err != nil || closeErr != nil {
		_ = root.Remove(name)
		return false
	}
	return true
}

func (q *operationQueue) run(c *Control) {
	defer close(q.done)
	root := operationRoot(c.retryDir)
	if root == nil {
		return
	}
	defer root.Close()
	var pending [operationSlots]*pendingOperation
	defer func() {
		for slot := range pending {
			_ = root.Remove(operationName(slot))
		}
	}()
	for slot := range pending {
		name := operationName(slot)
		pending[slot] = readOperation(root, name, q.scope)
		if pending[slot] == nil {
			_ = root.Remove(name)
		}
	}
	for q.ctx.Err() == nil {
		var next time.Time
		for _, item := range pending {
			if item != nil && (next.IsZero() || item.Next.Before(next)) {
				next = item.Next
			}
		}
		var tick chan struct{}
		var stop func()
		if !next.IsZero() {
			tick = make(chan struct{})
			stop = c.timer(time.Until(next), func() { close(tick) })
		}
		select {
		case <-q.ctx.Done():
			if stop != nil {
				stop()
			}
			return
		case item := <-q.actions:
			if stop != nil {
				stop()
			}
			if !item.valid(q.scope, time.Now()) {
				continue
			}
			for slot, current := range pending {
				if current == nil && writeOperation(root, operationName(slot), item) {
					pending[slot] = &item
					break
				}
			}
		case <-tick:
			for slot, item := range pending {
				if q.ctx.Err() != nil {
					return
				}
				if item == nil || item.Next.After(next) && time.Now().Before(item.Next) {
					continue
				}
				name := operationName(slot)
				// Claim before sending: a crash may lose this attempt but cannot
				// cause unlimited submissions under the same retry budget.
				if root.Remove(name) != nil {
					pending[slot] = nil
					continue
				}
				pending[slot] = nil
				if item.valid(q.scope, time.Now()) {
					delay := c.retryOperation(q, item)
					item.Retries++
					if delay == 0 || item.Retries >= operationRetries {
						continue
					}
					backoff := [operationRetries]time.Duration{operationInterval, 30 * time.Minute, 3 * time.Hour}
					item.Next = time.Now().UTC().Add(max(delay, backoff[item.Retries]))
					if item.valid(q.scope, time.Now()) && writeOperation(root, name, *item) {
						pending[slot] = item
					}
				}
			}
		}
	}
}

func (c *Control) retryOperation(q *operationQueue, item *pendingOperation) time.Duration {
	c.mu.Lock()
	if c.retry != q || !c.errorsLocked() || c.closing {
		c.mu.Unlock()
		return 0
	}
	event := c.eventLocked("operation", item.Fields, nil)
	event.EventID, event.Timestamp, event.Fingerprint[0], event.Breadcrumbs = item.Event, item.At, item.Fingerprint, nil
	c.mu.Unlock()
	_, delay := c.deliverReport(q.ctx, event)
	return delay
}
