package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"time"

	"charm.land/fantasy"
	acp "github.com/coder/acp-go-sdk"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// One agent session serves one Sailry session, as in T3 Code: it receives
// only the messages it has not seen, survives the loss of its program through
// `session/resume` or `session/load`, and shrinks its own context with the
// agent's `/compact` command where the session advertises one. A whole-history
// replay into a new session is the fallback.

// acpKeptSession records which agent session holds a Sailry session's history.
// The Node keeps it beside the package's program data, so a program started
// again after an idle stop, an eviction or a Node restart can resume it. It
// holds identifiers, digests and the session's model offers, never text.
type acpKeptSession struct {
	Session string     `json:"session"`
	System  string     `json:"system"`
	Seen    []string   `json:"seen"`
	Offers  []acpOffer `json:"offers"`
	Thought acpThought `json:"thought"`
	Compact bool       `json:"compact"`
}

// acpKeptCapacity bounds the records of one program; the least recently
// written leave first. A session whose record left replays when it continues.
const acpKeptCapacity = 256

func (model *acpModel) keptPath(identity string, session protocol.SessionID) string {
	if model.capture.Sessions == "" || session == (protocol.SessionID{}) {
		return ""
	}
	return filepath.Join(model.capture.Sessions, acpDigest(identity+"\x00"+session.String())+".json")
}

func (model *acpModel) recall(identity string, session protocol.SessionID) *acpKeptSession {
	path := model.keptPath(identity, session)
	if path == "" {
		return nil
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	var kept acpKeptSession
	if json.Unmarshal(raw, &kept) != nil || kept.Session == "" {
		return nil
	}
	return &kept
}

// keep stores the record of the slot's agent session, or forgets it when the
// session holds nothing that can continue. A record that cannot be written
// costs only a replay later.
func (model *acpModel) keep(agent *acpAgent, slot *acpSlot) {
	path := model.keptPath(agent.identity, slot.owner)
	if path == "" {
		return
	}
	agent.mu.Lock()
	compact := agent.commands[slot.session]["compact"]
	agent.mu.Unlock()
	if slot.session == "" || len(slot.seen) == 0 || !agent.resume && !agent.load {
		_ = os.Remove(path)
		return
	}
	raw, err := json.Marshal(acpKeptSession{Session: slot.session, System: acpDigest(slot.system), Seen: slot.seen,
		Offers: slot.offers, Thought: slot.thought, Compact: compact})
	if err != nil || os.MkdirAll(model.capture.Sessions, 0700) != nil {
		return
	}
	temporary, err := os.CreateTemp(model.capture.Sessions, ".kept-*")
	if err != nil {
		return
	}
	_, err = temporary.Write(raw)
	if closed := temporary.Close(); err != nil || closed != nil || os.Rename(temporary.Name(), path) != nil {
		_ = os.Remove(temporary.Name())
		return
	}
	model.prune()
}

func (model *acpModel) prune() {
	entries, err := os.ReadDir(model.capture.Sessions)
	if err != nil || len(entries) <= acpKeptCapacity {
		return
	}
	type record struct {
		name string
		used int64
	}
	var records []record
	for _, entry := range entries {
		if info, err := entry.Info(); err == nil && strings.HasSuffix(entry.Name(), ".json") {
			records = append(records, record{entry.Name(), info.ModTime().UnixNano()})
		}
	}
	sort.Slice(records, func(i, j int) bool { return records[i].used < records[j].used })
	for _, stale := range records[:max(0, len(records)-acpKeptCapacity)] {
		_ = os.Remove(filepath.Join(model.capture.Sessions, stale.name))
	}
}

// resume reopens, in a program that does not hold it, the agent session that
// holds a prefix of this history: by `session/resume` where the agent offers
// it, else, or when that fails, by `session/load`. The agent's replay of a
// loaded session is not forwarded, because no prompt is in flight. It reports
// false when neither succeeded; the caller then replays into a new session.
func (model *acpModel) resume(ctx context.Context, agent *acpAgent, slot *acpSlot, system string, digests []string, messages []protocol.PluginModelAdapterWireMessage) bool {
	kept := model.recall(agent.identity, slot.owner)
	if kept == nil || kept.Session == slot.session || kept.System != acpDigest(system) || !acpExtends(kept.Seen, digests, messages) {
		return false
	}
	params := map[string]any{"sessionId": kept.Session, "cwd": model.capture.Dir, "mcpServers": []any{}}
	var resumed acpSession
	err := errors.New("the agent cannot resume a session")
	if agent.resume {
		resumed, err = acpCall[acpSession](ctx, agent, acp.AgentMethodSessionResume, params)
	}
	if err != nil && agent.load {
		resumed, err = acpCall[acpSession](ctx, agent, acp.AgentMethodSessionLoad, params)
	}
	if err != nil {
		return false
	}
	offers, thought := acpOffers(resumed)
	if len(offers) == 0 {
		offers, thought = kept.Offers, kept.Thought
	}
	if kept.Compact {
		agent.mu.Lock()
		agent.advertise(kept.Session, "compact")
		agent.mu.Unlock()
	}
	slot.session, slot.offers, slot.thought = kept.Session, offers, thought
	// The session was told its system text when it was created.
	slot.system, slot.seen, slot.prefixed, slot.selected, slot.level = system, slices.Clone(kept.Seen), true, "", ""
	return true
}

// advertise records a command the agent offers in a session. The caller holds
// agent.mu.
func (agent *acpAgent) advertise(session, command string) {
	if agent.commands == nil {
		agent.commands = map[string]map[string]bool{}
	}
	if agent.commands[session] == nil {
		agent.commands[session] = map[string]bool{}
	}
	agent.commands[session][command] = true
}

// acpExtends reports whether a session that has seen seen can continue to
// messages, identified by digests, by receiving only the messages after it.
// An assistant message among those is a reply another model or program
// wrote; the session would read it as user text, so that history is replayed.
func acpExtends(seen, digests []string, messages []protocol.PluginModelAdapterWireMessage) bool {
	if len(seen) == 0 || len(seen) >= len(digests) || !slices.Equal(seen, digests[:len(seen)]) {
		return false
	}
	return !slices.ContainsFunc(messages[len(seen):], func(message protocol.PluginModelAdapterWireMessage) bool {
		_, assistant := message.Data.(*protocol.PluginModelAdapterWireMessageAssistantPayload)
		return assistant
	})
}

// acpHistory is the system text of a request and the digests that identify
// its messages to a session.
func acpHistory(request protocol.PluginModelAdapterWireRequest) (string, []string, error) {
	system, err := acpSystem(request)
	if err != nil {
		return "", nil, err
	}
	digests := make([]string, len(request.Messages))
	for index, message := range request.Messages {
		if reply, assistant := acpStoredReply(message); assistant {
			digests[index] = reply
		} else {
			digests[index] = acpDigest(message)
		}
	}
	return system, digests, nil
}

// route reports the prompt bound, whether a program holds or can resume the
// session's history, and whether that session advertises `compact`.
func (model *acpModel) route(session protocol.SessionID) Route {
	_, identity := model.launch(false)
	route := Route{PromptBytes: int(model.capture.Adapter.Acp.ReplayBytes), Session: true}
	model.capture.Pool.Each(model.capture.Owner, func(entry *program.Entry) {
		agent, ok := entry.Value.(*acpAgent)
		if !ok || agent.identity != identity || route.Resident {
			return
		}
		agent.mu.Lock()
		defer agent.mu.Unlock()
		slot := agent.slots[session]
		if slot == nil || slot.busy || slot.session == "" || len(slot.seen) == 0 {
			return
		}
		route.Compact = agent.commands[slot.session]["compact"]
		route.Resident = true
	})
	if !route.Resident {
		if kept := model.recall(identity, session); kept != nil {
			route.Resident, route.Compact = true, kept.Compact
		}
	}
	return route
}

// CompactSession asks an agent session that holds the history to shrink its
// own context, after Sailry recorded its handoff. call is the next main
// request over the compacted history. The command is sent only where the
// session advertised it and the compacted history retains the last messages
// the session has seen; afterwards the session counts the history up to them
// as seen, so the next request sends only what follows them. Otherwise
// nothing is sent and the next request starts a new session with the handoff.
func CompactSession(ctx context.Context, languageModel fantasy.LanguageModel, call fantasy.Call) error {
	model, ok := languageModel.(*acpModel)
	if !ok {
		return nil
	}
	request, err := (&pluginModel{binding: model.binding, capture: PluginCapture{Declaration: model.capture.Adapter}}).request(call)
	if err != nil {
		return err
	}
	system, digests, err := acpHistory(request)
	if err != nil {
		return err
	}
	entry, agent, err := model.lease(ctx, false)
	if err != nil {
		return err
	}
	keep := true
	defer func() { model.capture.Pool.Release(entry, keep) }()
	slot := model.reserve(ctx, agent)
	defer agent.give(slot)
	if slot.owner == (protocol.SessionID{}) || slot.session == "" || slot.system != system || len(slot.seen) == 0 {
		return nil
	}
	agent.mu.Lock()
	advertised := agent.commands[slot.session]["compact"]
	agent.mu.Unlock()
	// The retained messages are a run the session saw in this order, from the
	// first compacted message it has seen. What follows that run, such as a
	// tool result or user input that was not yet sent, is new to the session
	// even where an earlier message reads the same.
	seen := 0
	if start := slices.IndexFunc(digests, func(digest string) bool { return slices.Contains(slot.seen, digest) }); start >= 0 {
		for from, digest := range slot.seen {
			if digest != digests[start] {
				continue
			}
			end := start
			for end < len(digests) && from+end-start < len(slot.seen) && digests[end] == slot.seen[from+end-start] {
				end++
			}
			seen = max(seen, end)
		}
	}
	if !advertised || seen == 0 {
		return nil
	}
	slot.seen = nil
	model.keep(agent, slot)
	// The command is a prompt of this session alone: a cancelled one stops
	// only that prompt, and one the agent never answers fails as a reply's
	// silence does. The program keeps serving its other sessions.
	done := make(chan error, 1)
	go func() {
		_, err := acp.SendRequest[acpPrompted](agent.conn, context.WithoutCancel(ctx), acp.AgentMethodSessionPrompt, map[string]any{"sessionId": slot.session,
			"prompt": []any{map[string]any{"type": "text", "text": "/compact"}}})
		done <- err
	}()
	stall := time.NewTimer(model.capture.stall)
	defer stall.Stop()
	select {
	case err = <-done:
	case <-ctx.Done():
		keep = acpCancel(agent, slot.session, done, nil, nil)
		return ctx.Err()
	case <-stall.C:
		keep = false
		return &Failure{Kind: Network, Message: fmt.Sprintf("%s did not finish /compact in %d seconds", model.capture.Program.Command, int(model.capture.stall.Seconds()))}
	}
	if err != nil {
		return model.failure(ctx, agent, err)
	}
	slot.seen = slices.Clone(digests[:seen])
	model.keep(agent, slot)
	return nil
}
