package model

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxCooldownEntries          = 256
	defaultRateLimitCooldown    = 300 * time.Millisecond
	cooldownStaggerStep         = 250 * time.Millisecond
	maxCooldownStagger          = 2 * time.Second
	concurrencyRecoverSuccesses = 4
	concurrencyUnboundedLimit   = 8
	concurrencyClampDecay       = 5 * time.Minute
)

type cooldownEntry struct {
	until   time.Time
	waiters int
}

type credentialSlot struct {
	inFlight  int
	limit     int
	successes int
	lastClamp time.Time
	notify    chan struct{}
}

// Cooldowns coordinates rate-limit wait windows and adaptive in-flight
// concurrency slots across concurrent requests sharing the same upstream
// credential so sibling turns, subagents, summaries, and auxiliary model calls
// do not hammer a throttled account or exceed its concurrency limit.
type Cooldowns struct {
	mu      sync.Mutex
	entries map[string]cooldownEntry
	slots   map[string]*credentialSlot
}

func NewCooldowns() *Cooldowns {
	return &Cooldowns{entries: map[string]cooldownEntry{}, slots: map[string]*credentialSlot{}}
}

// Until returns the active cooldown deadline for key if it extends past now.
func (c *Cooldowns) Until(key string, now time.Time) (time.Time, bool) {
	if c == nil || key == "" {
		return time.Time{}, false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	entry, ok := c.entries[key]
	if !ok {
		return time.Time{}, false
	}
	if !entry.until.After(now) {
		delete(c.entries, key)
		return time.Time{}, false
	}
	return entry.until, true
}

// WaitWindow returns the active cooldown deadline for key and a bounded wakeup
// stagger so multiple concurrent waiters do not stampede the upstream gateway
// at the exact instant a shared Retry-After window expires.
func (c *Cooldowns) WaitWindow(key string, now time.Time) (time.Time, time.Duration, bool) {
	if c == nil || key == "" {
		return time.Time{}, 0, false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	entry, ok := c.entries[key]
	if !ok {
		return time.Time{}, 0, false
	}
	if !entry.until.After(now) {
		delete(c.entries, key)
		return time.Time{}, 0, false
	}
	stagger := min(time.Duration(entry.waiters)*cooldownStaggerStep, maxCooldownStagger)
	entry.waiters++
	c.entries[key] = entry
	return entry.until, stagger, true
}

// Observe extends key's cooldown deadline to at least until (capped to
// retryWaitBudget from now). It never shortens an existing longer cooldown.
func (c *Cooldowns) Observe(key string, until, now time.Time) {
	if c == nil || key == "" {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	c.observeLocked(key, until, now)
}

func (c *Cooldowns) observeLocked(key string, until, now time.Time) {
	if !until.After(now) {
		return
	}
	if capUntil := now.Add(retryWaitBudget); until.After(capUntil) {
		until = capUntil
	}
	if c.entries == nil {
		c.entries = map[string]cooldownEntry{}
	}
	if existing, ok := c.entries[key]; ok {
		if until.After(existing.until) {
			c.entries[key] = cooldownEntry{until: until}
		}
		return
	}
	if len(c.entries) >= maxCooldownEntries {
		for k, v := range c.entries {
			if !v.until.After(now) {
				delete(c.entries, k)
			}
		}
	}
	if len(c.entries) >= maxCooldownEntries {
		var oldestKey string
		var oldestUntil time.Time
		for k, v := range c.entries {
			if oldestKey == "" || v.until.Before(oldestUntil) {
				oldestKey, oldestUntil = k, v.until
			}
		}
		if oldestKey != "" {
			delete(c.entries, oldestKey)
		}
	}
	c.entries[key] = cooldownEntry{until: until}
}

// ObserveFailure records a rate-limit cooldown derived from failure for direct
// callers that do not run through Run's retry schedule.
func (c *Cooldowns) ObserveFailure(key string, failure *Failure, now time.Time) {
	if c == nil || key == "" || failure == nil || failure.Kind != RateLimited {
		return
	}
	delay := max(retryHint(failure, now), defaultRateLimitCooldown)
	c.Observe(key, now.Add(delay), now)
}

func (c *Cooldowns) slotLocked(key string) *credentialSlot {
	if c.slots == nil {
		c.slots = map[string]*credentialSlot{}
	}
	slot := c.slots[key]
	if slot != nil {
		return slot
	}
	if len(c.slots) >= maxCooldownEntries {
		for k, s := range c.slots {
			if s.inFlight == 0 && s.limit == 0 {
				delete(c.slots, k)
			}
		}
	}
	slot = &credentialSlot{}
	c.slots[key] = slot
	return slot
}

func broadcastSlotLocked(slot *credentialSlot) {
	if slot != nil && slot.notify != nil {
		close(slot.notify)
		slot.notify = nil
	}
}

// InFlight returns the number of active requests currently holding a slot for key.
func (c *Cooldowns) InFlight(key string) int {
	if c == nil || key == "" {
		return 0
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if slot := c.slots[key]; slot != nil {
		return slot.inFlight
	}
	return 0
}

// SlotLimit returns the current adaptive concurrency ceiling for key (0 = unbounded).
func (c *Cooldowns) SlotLimit(key string) int {
	if c == nil || key == "" {
		return 0
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if slot := c.slots[key]; slot != nil {
		return slot.limit
	}
	return 0
}

// AcquireSlot obtains an in-flight concurrency permit for key when adaptive
// throttling is active, waiting up to maxWait for an active sibling to finish.
// Unthrottled credentials acquire immediately with zero wait.
func (c *Cooldowns) AcquireSlot(ctx context.Context, key string, now time.Time, maxWait time.Duration) (release func(), ok bool, err error) {
	if c == nil || key == "" {
		return func() {}, true, nil
	}
	var budgetTimer *time.Timer
	var budgetCh <-chan time.Time
	defer func() {
		if budgetTimer != nil {
			budgetTimer.Stop()
		}
	}()
	for {
		c.mu.Lock()
		slot := c.slotLocked(key)
		if slot.limit > 0 && slot.inFlight == 0 && !slot.lastClamp.IsZero() && now.Sub(slot.lastClamp) >= concurrencyClampDecay {
			slot.limit = 0
			slot.successes = 0
		}
		if slot.limit == 0 || slot.inFlight < slot.limit {
			slot.inFlight++
			c.mu.Unlock()
			return sync.OnceFunc(func() {
				c.mu.Lock()
				if s := c.slots[key]; s != nil {
					if s.inFlight > 0 {
						s.inFlight--
					}
					broadcastSlotLocked(s)
				}
				c.mu.Unlock()
			}), true, nil
		}
		if maxWait <= 0 {
			c.mu.Unlock()
			return nil, false, nil
		}
		if slot.notify == nil {
			slot.notify = make(chan struct{})
		}
		notify := slot.notify
		c.mu.Unlock()
		if budgetTimer == nil {
			budgetTimer = time.NewTimer(maxWait)
			budgetCh = budgetTimer.C
		}
		select {
		case <-notify:
			now = time.Now()
		case <-ctx.Done():
			return nil, false, ctx.Err()
		case <-budgetCh:
			return nil, false, nil
		}
	}
}

// ObserveThrottle records a rate-limit or overload rejection and adapts the
// credential's in-flight concurrency ceiling when concurrent requests collided.
// Call with the failed attempt still holding its slot, so a waiting sibling
// cannot wake before the shared Retry-After and the new ceiling are published.
func (c *Cooldowns) ObserveThrottle(key string, failure *Failure, now time.Time, retryDelay time.Duration) {
	if c == nil || key == "" || failure == nil {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	slot := c.slotLocked(key)
	siblings := max(0, slot.inFlight-1)
	delay := retryHint(failure, now)
	if failure.concurrencyLimited && siblings > 0 {
		// The local ceiling already queues colliding requests. Do not impose
		// exponential backoff on other requests beyond the provider's hint.
		if delay == 0 {
			delay = defaultRateLimitCooldown
		}
	} else {
		delay = max(delay, retryDelay)
	}
	c.observeLocked(key, now.Add(delay), now)
	// Clamp immediately on explicit concurrency limits, or on 429/overload when
	// sibling requests are actually running concurrently on this credential.
	if failure.concurrencyLimited || siblings > 0 && (failure.Kind == RateLimited || failure.overloaded) {
		target := max(1, siblings)
		if slot.limit == 0 || target < slot.limit {
			slot.limit = target
		}
		slot.successes = 0
		slot.lastClamp = now
	}
}

// ObserveSuccess gently probes an adapted concurrency ceiling back upward after
// sustained successful responses.
func (c *Cooldowns) ObserveSuccess(key string, now time.Time) {
	if c == nil || key == "" {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	slot := c.slots[key]
	if slot == nil || slot.limit == 0 {
		return
	}
	slot.successes++
	if slot.successes >= concurrencyRecoverSuccesses {
		slot.successes = 0
		slot.limit++
		if slot.limit >= concurrencyUnboundedLimit {
			slot.limit = 0
		}
		broadcastSlotLocked(slot)
	}
}

// Wait blocks until key's active cooldown expires or ctx ends.
func (c *Cooldowns) Wait(ctx context.Context, timer clock.Clock, key string) error {
	if c == nil || key == "" {
		return nil
	}
	if timer == nil {
		timer = clock.System{}
	}
	var waited time.Duration
	var lastUntil time.Time
	for {
		now := timer.Now()
		var until time.Time
		var stagger time.Duration
		var active bool
		if lastUntil.IsZero() {
			until, stagger, active = c.WaitWindow(key, now)
		} else {
			until, active = c.Until(key, now)
			if active && !until.Equal(lastUntil) {
				until, stagger, active = c.WaitWindow(key, now)
			}
		}
		if !active || !until.After(now) {
			return nil
		}
		lastUntil = until
		base := until.Sub(now)
		deadline, bounded := ctx.Deadline()
		if base > retryWaitBudget-waited || bounded && base >= deadline.Sub(now) {
			return &Failure{Kind: RateLimited, Message: "Rate limit cooldown exceeds remaining retry wait budget", WaitExhausted: true}
		}
		headroom := retryWaitBudget - waited - base
		if bounded {
			headroom = min(headroom, deadline.Sub(now)-base-time.Nanosecond)
		}
		delay := base + max(0, min(stagger, headroom))
		if err := timer.Sleep(ctx, delay); err != nil {
			return Classify(err)
		}
		if err := ctx.Err(); err != nil {
			return Classify(err)
		}
		waited += delay
	}
}

// CredentialCooldownKey derives a secret-safe identifier grouping requests that
// consume the same upstream credential and rate-limit scope.
func CredentialCooldownKey(binding Binding) string {
	origin := cooldownOrigin(binding.Provider)
	ref := binding.Config.Credential
	if ref == nil {
		ref = binding.Provider.Credential
	}
	credential := "provider:" + binding.Provider.ID.String()
	// API keys can be replaced in place. Account tokens can rotate while the
	// same account is still throttled, so retain its credential reference.
	if binding.Secret != nil && binding.Secret.Expose() != "" && (binding.Provider.Authentication == protocol.AuthenticationApiKey || ref == nil) {
		sum := sha256.Sum256([]byte(binding.Secret.Expose()))
		credential = "secret:" + hex.EncodeToString(sum[:16])
	} else if ref != nil {
		credential = "cred:" + ref.Node.String() + ":" + ref.ID.String()
	}
	if origin == "" {
		return credential
	}
	return origin + "|" + credential
}

func cooldownOrigin(provider protocol.ConversationProvider) string {
	if u, err := endpoint(provider.Endpoint); err == nil && u != nil {
		return strings.ToLower(u.Scheme) + "://" + strings.ToLower(u.Host)
	}
	if provider.Api == protocol.ConversationModelApiBedrock || provider.Api == protocol.ConversationModelApiVertexAi {
		return string(provider.Api) + ":" + provider.ID.String()
	}
	if strings.TrimSpace(provider.Endpoint) != "" {
		return strings.ToLower(strings.TrimSpace(provider.Endpoint))
	}
	return provider.ID.String()
}

type cooldownContextKey struct{}

type cooldownScope struct {
	store *Cooldowns
	key   string
}

// WithCooldown binds a shared Cooldowns tracker and credential key to ctx for Run.
func WithCooldown(ctx context.Context, store *Cooldowns, key string) context.Context {
	if store == nil || key == "" {
		return ctx
	}
	return context.WithValue(ctx, cooldownContextKey{}, cooldownScope{store: store, key: key})
}

func cooldownFromContext(ctx context.Context) (*Cooldowns, string) {
	if ctx == nil {
		return nil, ""
	}
	scope, ok := ctx.Value(cooldownContextKey{}).(cooldownScope)
	if !ok || scope.store == nil || scope.key == "" {
		return nil, ""
	}
	return scope.store, scope.key
}
