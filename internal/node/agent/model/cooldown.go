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
	maxCooldownEntries       = 256
	defaultRateLimitCooldown = 300 * time.Millisecond
	cooldownStaggerStep      = 250 * time.Millisecond
	maxCooldownStagger       = 2 * time.Second
)

type cooldownEntry struct {
	until   time.Time
	waiters int
}

// Cooldowns coordinates rate-limit wait windows across concurrent requests
// sharing the same upstream credential so sibling turns, subagents, summaries,
// and auxiliary model calls do not hammer a throttled account during an active
// HTTP 429 cooldown.
type Cooldowns struct {
	mu      sync.Mutex
	entries map[string]cooldownEntry
}

func NewCooldowns() *Cooldowns {
	return &Cooldowns{entries: map[string]cooldownEntry{}}
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
	if c == nil || key == "" || !until.After(now) {
		return
	}
	if capUntil := now.Add(retryWaitBudget); until.After(capUntil) {
		until = capUntil
	}
	c.mu.Lock()
	defer c.mu.Unlock()
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

// Clear drops all active cooldowns (e.g. after provider or credential changes).
func (c *Cooldowns) Clear() {
	if c == nil {
		return
	}
	c.mu.Lock()
	clear(c.entries)
	c.mu.Unlock()
}

// CredentialCooldownKey derives a secret-safe identifier grouping requests that
// consume the same upstream credential and rate-limit scope.
func CredentialCooldownKey(binding Binding) string {
	origin := cooldownOrigin(binding.Provider)
	var credential string
	switch {
	case binding.Config.Credential != nil:
		credential = "cred:" + binding.Config.Credential.Node.String() + ":" + binding.Config.Credential.ID.String()
	case binding.Provider.Credential != nil:
		credential = "cred:" + binding.Provider.Credential.Node.String() + ":" + binding.Provider.Credential.ID.String()
	case binding.Secret != nil && binding.Secret.Expose() != "":
		sum := sha256.Sum256([]byte(binding.Secret.Expose()))
		credential = "secret:" + hex.EncodeToString(sum[:16])
	default:
		credential = "provider:" + binding.Provider.ID.String()
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
