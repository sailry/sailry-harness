package preview

import (
	"fmt"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// The reply the preview plays after a send: a fixed script on the clock
// the view passes in, so tests step it with the tester's time. No model is
// asked; the reply is canned text.

// Script times, from the send.
const (
	thinkFor  = 600 * time.Millisecond
	readAt    = 600 * time.Millisecond
	readFor   = 700 * time.Millisecond
	answerAt  = 1500 * time.Millisecond
	charEvery = 15 * time.Millisecond // the answer grows a character at a time
)

// Tick is how often a playing reply changes: the view asks for a frame
// this far ahead while Playing.
const Tick = 50 * time.Millisecond

const answer = "The preview played this reply without a model. It grows a few characters at a time, as a streamed answer does, so the transcript can be checked to follow its end.\n\n- The work region stays open while it runs.\n- It folds once the run completes.\n\nScroll up while it plays and the jump button shows; click it to follow again."

// script is the reply a send plays: the file it reads and the answer it
// writes.
type script struct {
	label, file, output, answer string
}

var previewScript = script{label: "Read file", file: "README.md", output: "# sailry-go\n\nA Go prototype of Sailry's desktop UI.", answer: answer}

type stream struct {
	start time.Time
	turn  int
}

// Send adds a turn for text and starts playing its reply at now. The user's
// turn takes the background results that were held and lets a paused queue
// run again.
func Send(c *Conversation, text string, now time.Time) {
	c.Held = nil
	resume(c)
	id := fmt.Sprintf("%s/sent-%d", c.ID, len(c.Turns))
	c.Turns = append(c.Turns, cv.Turn{ID: id, User: text, Reply: cv.Reply{Status: cv.Queued}})
	c.Animating = true
	simulation(c).stream = &stream{start: now, turn: len(c.Turns) - 1}
	c.Status, c.Updated = Running, Now
}

// Advance moves the playing reply to where it is at now.
func Advance(c *Conversation, now time.Time) {
	page(c, now)
	s := simulation(c).stream
	if s == nil {
		return
	}
	t := &c.Turns[s.turn]
	play := &previewScript
	if own := simulation(c).script; own != nil {
		play = own
	}
	d := now.Sub(s.start)
	r := cv.Reply{Status: cv.Running, Elapsed: d.Truncate(time.Second), Phase: cv.Thinking}
	if d >= readAt {
		read := cv.Call{ID: t.ID + "/read", Name: "read", Label: play.label, Icon: "book-open", Detail: play.file, Code: true}
		if d < readAt+readFor {
			read.Running = true
		} else {
			read.Output = &cv.Output{Text: play.output}
		}
		r.Blocks = append(r.Blocks, cv.Block{Kind: cv.ToolsBlock, Calls: []cv.Call{read}})
	}
	if d >= answerAt {
		answer := []rune(play.answer)
		n := min(int((d-answerAt)/charEvery), len(answer))
		r.Phase = cv.Generating
		r.Blocks = append(r.Blocks, cv.Block{Kind: cv.TextBlock, Text: string(answer[:n])})
		if n == len(answer) {
			r.Status, r.Phase = cv.Completed, cv.PhaseNone
			simulation(c).stream, c.Status, c.Animating = nil, Idle, false
			release(c)
		}
	}
	t.Reply = r
}

// Stop stops the reply: the played one, or the static one.
func Stop(c *Conversation) {
	n := len(c.Turns)
	if n == 0 || !c.Turns[n-1].Reply.Status.Active() {
		return
	}
	r := &c.Turns[n-1].Reply
	r.Status, r.Phase = cv.Cancelled, cv.PhaseNone
	simulation(c).stream, c.Status, c.Waiting, c.Animating = nil, Idle, composer.Interactions{}, false
	release(c)
	// Stop pauses the queue: what waited in order waits for the user.
	for i := range c.Queue {
		if c.Queue[i].Wait == composer.WaitNone {
			c.Queue[i].Wait = composer.WaitStopped
		}
	}
}

// release returns the guidance the ended reply did not take to the queue.
func release(c *Conversation) {
	for i := range c.Queue {
		c.Queue[i].Guided = false
	}
}

// Decide applies a preview decision to the approval's call: it runs, or is
// denied. Nothing runs either way.
func Decide(c *Conversation, id string, d composer.Decision) {
	var rest []composer.Approval
	for _, a := range c.Waiting.Approvals {
		if a.ID != id {
			rest = append(rest, a)
		}
	}
	c.Waiting.Approvals = rest
	eachCall(c, id, func(call *cv.Call) {
		call.Pending, call.State = false, cv.StateNone
		if d == composer.Deny {
			call.Warning, call.State = true, cv.Denied
		}
	})
}

// Resolve closes the pending question: answered with answers, or cancelled
// or declined with none.
func Resolve(c *Conversation, id string, st cv.QuestionState, answers []string) {
	c.Waiting.Question = nil
	for i := range c.Turns {
		for j := range c.Turns[i].Reply.Blocks {
			if q := c.Turns[i].Reply.Blocks[j].Question; q != nil && q.CallID == id {
				q.State, q.Answers = st, answers
			}
		}
	}
}

func eachCall(c *Conversation, id string, fn func(*cv.Call)) {
	for i := range c.Turns {
		for j := range c.Turns[i].Reply.Blocks {
			b := &c.Turns[i].Reply.Blocks[j]
			for k := range b.Calls {
				if b.Calls[k].ID == id {
					fn(&b.Calls[k])
				}
			}
		}
	}
}
