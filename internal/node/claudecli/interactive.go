package claudecli

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
)

// Interactive mode runs Claude Code's own terminal interface in a
// pseudo-terminal: a message is pasted and sent with Enter, the reply is read
// from the transcript Claude Code writes for the session, and a tool call is
// the request its MCP client makes. Nothing is loaded into the binary. The
// screen texts and the transcript rows are those magpie 0af73f4 records
// (internal/claudetui); none was checked against a real Claude Code, so the
// mode is experimental and off unless the user turns it on.

const (
	screenColumns = 200
	screenRows    = 50
	// screenKept bounds what is remembered of the drawn output.
	screenKept = 64 << 10
	// followEvery is how often the transcript is read for new rows.
	followEvery = 50 * time.Millisecond
	// sendTries is how often Enter is pressed for one message.
	sendTries = 4
)

// The waits of the terminal interface, variables so that a test can shorten them.
var (
	// readyWait bounds the start: the prompt drawn and the tools listed.
	readyWait = 45 * time.Second
	// readySettle lets a drawn prompt begin to listen.
	readySettle = 300 * time.Millisecond
	// pasteSettle lets a paste be taken in before Enter.
	pasteSettle = 150 * time.Millisecond
	// sentWait is how long one Enter may take to reach the transcript.
	sentWait = 3 * time.Second
)

var (
	escapes = regexp.MustCompile(`\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[P^_][^\x1b]*\x1b\\|\x1b.`)
	// signInScreen is what the interface draws where it is not signed in.
	signInScreen = regexp.MustCompile(`Select login method|Log in with your Claude account|claude\.ai/oauth/authorize`)
	// firstRunScreen is one of the screens a Claude Code that was never run shows.
	firstRunScreen = regexp.MustCompile(`Security notes:.*Press Enter to continue|Choose the text style|terminal setup\?.*Esc to skip`)
)

const (
	trustScreen = "trust this folder"
	promptMark  = "❯"
)

// InteractiveArguments is the command line of the terminal interface: the
// print mode's without what only print mode takes, under a session identity
// of Sailry's so that its transcript can be found.
func InteractiveArguments(settings Settings, session string) []string {
	arguments := append([]string{"--session-id", session, "--no-chrome"}, shared(settings)...)
	if settings.Effort != "" {
		arguments = append(arguments, "--effort", settings.Effort)
	}
	return arguments
}

// screen is what the child drew, as a stream with its escapes taken out.
type screen struct {
	mu      sync.Mutex
	drawn   []byte
	changed chan struct{}
}

// watch reads the terminal until the child ends. Claude Code asks the
// terminal where its cursor is and what it is; both are answered, as a
// terminal would, or its interface waits for them.
func (screen *screen) watch(terminal io.ReadWriter) {
	buffer := make([]byte, 32<<10)
	for {
		count, err := terminal.Read(buffer)
		if count > 0 {
			drawn := buffer[:count]
			if bytes.Contains(drawn, []byte("\x1b[6n")) {
				_, _ = io.WriteString(terminal, "\x1b[1;1R")
			}
			if bytes.Contains(drawn, []byte("\x1b[c")) || bytes.Contains(drawn, []byte("\x1b[0c")) {
				_, _ = io.WriteString(terminal, "\x1b[?62;c")
			}
			screen.mu.Lock()
			screen.drawn = append(screen.drawn, drawn...)
			if len(screen.drawn) > screenKept {
				screen.drawn = append([]byte(nil), screen.drawn[len(screen.drawn)-screenKept/2:]...)
			}
			screen.mu.Unlock()
			select {
			case screen.changed <- struct{}{}:
			default:
			}
		}
		if err != nil {
			return
		}
	}
}

// text is the drawn output as words separated by single spaces.
func (screen *screen) text() string {
	screen.mu.Lock()
	drawn := string(screen.drawn)
	screen.mu.Unlock()
	return strings.Join(strings.Fields(escapes.ReplaceAllString(drawn, " ")), " ")
}

// last is the end of the drawn output, for a failure to quote.
func (screen *screen) last() string {
	text := screen.text()
	if len(text) > 400 {
		text = text[len(text)-400:]
		for len(text) > 0 && !utf8.RuneStart(text[0]) {
			text = text[1:]
		}
	}
	return text
}

// ready waits until the interface can take a message: its prompt is drawn
// and, where tools are offered, it has listed them. A question only the user
// may answer ends the start with what the user has to do; Sailry presses no
// key on such a screen.
func (value *routeModel) ready(ctx context.Context, started *run) error {
	deadline := time.NewTimer(readyWait)
	defer deadline.Stop()
	prompted := false
	for {
		text := started.screen.text()
		switch {
		case strings.Contains(text, trustScreen):
			return &model.Failure{Kind: model.NotConfigured, Message: fmt.Sprintf("Claude Code asks whether to trust the folder %s. Run claude once in that folder and answer it", value.capture.Dir)}
		case signInScreen.MatchString(text):
			return value.failure("Not logged in", 0)
		case firstRunScreen.MatchString(text):
			return &model.Failure{Kind: model.NotConfigured, Message: "Claude Code has not finished its first-run screens. Run claude once in a terminal and go through them"}
		case strings.Contains(text, promptMark):
			// A prompt drawn before the tools are listed would answer without them.
			prompted = true
			if started.bridge == nil || started.bridge.listedNow() {
				select {
				case <-time.After(readySettle):
					return nil
				case <-ctx.Done():
					return stopped()
				}
			}
		}
		select {
		case <-started.screen.changed:
		case <-time.After(200 * time.Millisecond):
		case <-ctx.Done():
			return stopped()
		case <-started.process.Done():
			return value.ended(started)
		case <-deadline.C:
			state := "showed no prompt"
			if prompted {
				state = "showed its prompt and did not list Sailry's tools"
			}
			return &model.Failure{Kind: model.Unavailable, Message: fmt.Sprintf("Claude Code %s in %d seconds: %s", state, int(readyWait.Seconds()), started.screen.last())}
		}
	}
}

// pasteable is text as a bracketed paste can carry it: no escape that would
// end the paste early and no other control character, and not opening with
// what the prompt takes as a command, a shell line, a memory or a background
// task.
func pasteable(text string) string {
	text = strings.Map(func(character rune) rune {
		switch {
		case character == '\r':
			return '\n'
		case character == '\n', character == '\t':
			return character
		case character < 0x20, character == 0x7f:
			return -1
		}
		return character
	}, strings.ReplaceAll(text, "\r\n", "\n"))
	if opening := strings.TrimLeft(text, " \t\n"); opening != "" && strings.ContainsRune("/!#&", rune(opening[0])) {
		text = "\n" + opening
	}
	if strings.TrimSpace(text) == "" {
		text = "."
	}
	return text
}

// send pastes one message into the prompt and presses Enter, again while the
// message has not reached the transcript: an Enter that lands while the paste
// is still being taken in becomes part of it.
func (value *routeModel) send(ctx context.Context, current *run, blocks []map[string]any) error {
	text := ""
	for _, block := range blocks {
		part, _ := block["text"].(string)
		text += part
	}
	before := current.sent()
	if _, err := io.WriteString(current.process.Stdin, "\x1b[200~"+pasteable(text)+"\x1b[201~"); err != nil {
		return value.ended(current)
	}
	wait := pasteSettle
	for try := 0; try < sendTries; try++ {
		select {
		case <-time.After(wait):
		case <-ctx.Done():
			return stopped()
		case <-current.process.Done():
			return value.ended(current)
		}
		if _, err := io.WriteString(current.process.Stdin, "\r"); err != nil {
			return value.ended(current)
		}
		taken := time.NewTimer(sentWait)
		for waiting := true; waiting; {
			if current.sent() > before {
				taken.Stop()
				return nil
			}
			select {
			case <-current.rows:
			case <-taken.C:
				waiting = false
			case <-ctx.Done():
				taken.Stop()
				return stopped()
			case <-current.process.Done():
				taken.Stop()
				return value.ended(current)
			}
		}
		wait = 0
	}
	return &model.Failure{Kind: model.Unavailable, Message: "Claude Code did not take the message: " + current.screen.last()}
}

// sent counts the user messages the transcript holds.
func (run *run) sent() int {
	run.mu.Lock()
	defer run.mu.Unlock()
	return run.prompts
}

// transcripts is where Claude Code keeps its sessions' transcripts: under
// the configuration folder the user's environment names, else in the home
// folder. Sailry reads only the transcript of a session it started.
func transcripts(host []string) string {
	configured, home := "", ""
	for _, entry := range host {
		switch name, text, _ := strings.Cut(entry, "="); name {
		case "CLAUDE_CONFIG_DIR":
			configured = text
		case "HOME", "USERPROFILE":
			if home == "" || name == "HOME" {
				home = text
			}
		}
	}
	if configured == "" {
		configured = filepath.Join(home, ".claude")
	}
	return filepath.Join(configured, "projects")
}

// arrival is a tool call Claude Code made through the bridge.
type arrival struct {
	identity, name string
	arguments      json.RawMessage
}

// follow tells the step what the session's transcript says and which tool
// calls arrive, until the child ends. It is the only writer of the events.
func (run *run) follow(projects, session string) {
	defer close(run.events)
	translator := &translator{told: map[string]bool{}}
	var reader *bufio.Reader
	var file *os.File
	defer func() {
		if file != nil {
			_ = file.Close()
		}
	}()
	var part []byte
	emit := func(events []event) bool {
		if len(events) == 0 {
			return true
		}
		select {
		case run.events <- events:
			return true
		case <-run.done:
			return false
		}
	}
	read := func() bool {
		if file == nil {
			// The folder is named after the working folder by Claude Code's own rule.
			matches, _ := filepath.Glob(filepath.Join(projects, "*", session+".jsonl"))
			if len(matches) == 0 {
				return true
			}
			opened, err := os.Open(matches[0])
			if err != nil {
				return true
			}
			file, reader = opened, bufio.NewReaderSize(opened, 1<<20)
		}
		for {
			line, err := reader.ReadBytes('\n')
			part = append(part, line...)
			if len(part) == 0 || part[len(part)-1] != '\n' {
				// A row still being written is read again when it is whole.
				return err == nil || errors.Is(err, io.EOF)
			}
			events, prompt := translator.row(part)
			part = nil
			if prompt {
				run.mu.Lock()
				run.prompts++
				run.mu.Unlock()
				select {
				case run.rows <- struct{}{}:
				default:
				}
			}
			if !emit(events) {
				return false
			}
		}
	}
	for {
		if !read() {
			return
		}
		select {
		case call := <-run.calls:
			// What the transcript already holds was said before the call.
			if !read() || !emit(translator.call(call)) {
				return
			}
		case <-time.After(followEvery):
		case <-run.process.Done():
			read()
			return
		case <-run.done:
			return
		}
	}
}

// translator turns transcript rows into step events. Claude Code writes one
// row per content block of an assistant message, under the message's
// identity, an API error as a synthetic message and a turn's end as a
// turn_duration row. A message has no row until it is complete, so nothing
// streams. Its text is held until the message is known to be the current
// reply: a message that calls a tool may be written only after the call
// returned, and its text is then dropped, as the call was already told.
type translator struct {
	message string
	blocks  int
	held    []event
	// told are the tool calls the bridge reported.
	told   map[string]bool
	failed bool
}

type transcriptRow struct {
	Type        string `json:"type"`
	Subtype     string `json:"subtype"`
	IsSidechain bool   `json:"isSidechain"`
	IsMeta      bool   `json:"isMeta"`
	IsAPIError  bool   `json:"isApiErrorMessage"`
	APIStatus   int    `json:"apiErrorStatus"`
	APIParams   struct {
		RateLimitInfo *struct {
			Status   string `json:"status"`
			ResetsAt int64  `json:"resetsAt"`
		} `json:"rate_limit_info"`
	} `json:"apiErrorParams"`
	Message json.RawMessage `json:"message"`
}

func (translator *translator) flush() []event {
	held := translator.held
	translator.held = nil
	return held
}

// call tells a tool call the bridge received: the step ends on it.
func (translator *translator) call(call arrival) []event {
	translator.told[call.identity] = true
	arguments := call.arguments
	if len(arguments) == 0 {
		arguments = json.RawMessage("{}")
	}
	return append(translator.flush(),
		event{kind: eventCall, message: translator.message, identity: call.identity, name: toolPrefix + call.name, arguments: arguments},
		event{kind: eventMessageEnd, message: translator.message})
}

// row translates one transcript row. prompt reports a message the user sent.
func (translator *translator) row(raw []byte) (events []event, prompt bool) {
	var row transcriptRow
	if json.Unmarshal(raw, &row) != nil || row.IsSidechain {
		return nil, false
	}
	switch row.Type {
	case "user":
		if !row.IsMeta && typed(row.Message) {
			translator.failed = false
			return nil, true
		}
	case "assistant":
		var message struct {
			ID         string      `json:"id"`
			Model      string      `json:"model"`
			StopReason string      `json:"stop_reason"`
			Usage      wireUsage   `json:"usage"`
			Content    []wireBlock `json:"content"`
		}
		if json.Unmarshal(row.Message, &message) != nil {
			return nil, false
		}
		if message.Model == synthetic {
			if !row.IsAPIError {
				return nil, false
			}
			text := ""
			for _, block := range message.Content {
				if block.Type == "text" {
					text += block.Text
				}
			}
			translator.failed = true
			events = translator.flush()
			if limit := row.APIParams.RateLimitInfo; limit != nil {
				limited := event{kind: eventLimit, rejected: limit.Status == "rejected"}
				if limit.ResetsAt > 0 {
					limited.resets = time.Unix(limit.ResetsAt, 0)
				}
				events = append(events, limited)
			}
			return append(events, event{kind: eventResult, failed: true, text: text, status: row.APIStatus}), false
		}
		if message.ID == "" {
			return nil, false
		}
		if message.ID != translator.message {
			events = append(translator.flush(), event{kind: eventMessage, message: message.ID})
			translator.message, translator.blocks = message.ID, 0
		}
		events = append(events, event{kind: eventUsage, message: message.ID, usage: message.Usage.usage(), stop: message.StopReason})
		for _, block := range message.Content {
			switch block.Type {
			case "text":
				translator.held = append(translator.held, event{kind: eventText, index: translator.blocks, message: message.ID, text: block.Text})
			case "thinking":
				translator.held = append(translator.held, event{kind: eventReasoning, index: translator.blocks, message: message.ID, text: block.Thinking},
					event{kind: eventReasoningEnd, index: translator.blocks, message: message.ID})
			case "tool_use":
				if translator.told[block.ID] {
					translator.held = nil
				} else {
					events = append(events, translator.flush()...)
				}
			}
			translator.blocks++
		}
		return events, false
	case "system":
		if row.Subtype == "turn_duration" {
			events = translator.flush()
			if !translator.failed {
				events = append(events, event{kind: eventResult})
			}
			translator.failed = false
			return events, false
		}
	}
	return nil, false
}

// typed reports whether a user row's message was typed in: text, not a
// tool's result.
func typed(raw json.RawMessage) bool {
	var message struct {
		Content json.RawMessage `json:"content"`
	}
	if json.Unmarshal(raw, &message) != nil || len(message.Content) == 0 {
		return false
	}
	if message.Content[0] == '"' {
		return true
	}
	var blocks []struct {
		Type string `json:"type"`
	}
	if json.Unmarshal(message.Content, &blocks) != nil {
		return false
	}
	for _, block := range blocks {
		if block.Type == "tool_result" {
			return false
		}
	}
	return len(blocks) > 0
}
