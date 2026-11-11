package telemetry

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"runtime/debug"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/getsentry/sentry-go"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/logging"
)

// The interface's two failures are kept in the application's log directory
// as reduced traces: every goroutine's state, and each frame's function name
// and line number. A reduced trace has no panic message, no argument, no
// file path and no register.
const (
	HangFile  = "interface-hang.txt"
	CrashFile = "interface-crash.txt"
	// crashOutput receives the runtime's own traceback of a fatal panic. It
	// is reduced and removed at the next launch and never leaves the device.
	crashOutput = "interface-crash.raw"

	watchInterval = time.Second
	// HangAfter is how long the interface thread may leave a heartbeat
	// unanswered before it counts as hung.
	HangAfter = 5 * time.Second
	// watchLinger is how long the heartbeat continues after the interface's
	// last frame. An idle interface is not woken at all.
	watchLinger = 30 * time.Second
	stackLimit  = 4 << 20
)

// crashClasses are the only words a crash's cause is reported in.
var crashClasses = []struct{ prefix, contains, class string }{
	{"panic: ", "nil pointer dereference", "panic.nil_dereference"},
	{"panic: ", "index out of range", "panic.index_out_of_range"},
	{"panic: ", "slice bounds out of range", "panic.slice_out_of_range"},
	{"panic: ", "integer divide by zero", "panic.divide_by_zero"},
	{"panic: ", "interface conversion", "panic.type_assertion"},
	{"panic: ", "nil map", "panic.nil_map"},
	{"panic: ", "closed channel", "panic.closed_channel"},
	{"panic: ", "runtime error", "panic.runtime_error"},
	{"panic: ", "", "panic.other"},
	{"fatal error: ", "concurrent map", "fatal.concurrent_map"},
	{"fatal error: ", "all goroutines are asleep", "fatal.deadlock"},
	{"fatal error: ", "out of memory", "fatal.out_of_memory"},
	{"fatal error: ", "stack overflow", "fatal.stack_overflow"},
	{"fatal error: ", "", "fatal.other"},
	{"[signal ", "", "signal"},
	{"SIG", "", "signal"},
}

// reduce rewrites a Go traceback as goroutine headers and "function:line"
// frames. Whatever precedes the first goroutine, a panic's message included,
// becomes one word of crashClasses.
func reduce(raw []byte) []byte {
	var out bytes.Buffer
	class, started := "", false
	lines := strings.Split(string(raw), "\n")
	for index := 0; index < len(lines); index++ {
		line := strings.TrimRight(lines[index], "\r")
		if rest, found := strings.CutPrefix(line, "goroutine "); found && strings.HasSuffix(line, "]:") {
			if !started && class != "" {
				out.WriteString(class + "\n")
			}
			started = true
			out.WriteString("goroutine")
			id, _, _ := strings.Cut(rest, " ")
			if _, err := strconv.ParseUint(id, 10, 64); err == nil {
				out.WriteString(" " + id)
			}
			if at := strings.LastIndex(rest, "["); at >= 0 {
				if state := strings.TrimSuffix(rest[at+1:], "]:"); words(state) {
					out.WriteString(" [" + state + "]")
				}
			}
			out.WriteByte('\n')
			continue
		}
		if !started {
			// A panic raised while another unwinds follows it, indented:
			// the last one named is the one that ended the process.
			header := strings.TrimSpace(line)
			for _, known := range crashClasses {
				if strings.HasPrefix(header, known.prefix) && strings.Contains(header, known.contains) {
					if class == "" || known.class != "signal" {
						class = known.class
					}
					break
				}
			}
			continue
		}
		if line == "" {
			out.WriteByte('\n')
			continue
		}
		if line[0] == '\t' || line[0] == ' ' {
			continue
		}
		name, prefix := line, ""
		if rest, found := strings.CutPrefix(line, "created by "); found {
			name, _, _ = strings.Cut(rest, " in goroutine ")
			prefix = "created by "
		} else if at := strings.LastIndexByte(line, '('); at > 0 {
			name = line[:at]
		} else {
			// Registers and notes that follow a signal's goroutines.
			continue
		}
		if strings.ContainsRune(name, ' ') {
			continue
		}
		if !symbol(name) || strings.ContainsAny(name, ":,;!?'") {
			name = "unknown"
		}
		number := "0"
		if index+1 < len(lines) && strings.HasPrefix(lines[index+1], "\t") {
			// "<file>:<line> +0x<offset>"; the file may hold spaces.
			location := strings.TrimRight(lines[index+1], "\r")
			if at := strings.LastIndex(location, " +0x"); at >= 0 {
				location = location[:at]
			}
			if at := strings.LastIndexByte(location, ':'); at >= 0 {
				if _, err := strconv.ParseUint(strings.TrimSpace(location[at+1:]), 10, 31); err == nil {
					number = strings.TrimSpace(location[at+1:])
				}
			}
		}
		out.WriteString(prefix + name + ":" + number + "\n")
	}
	if !started && class != "" {
		out.WriteString(class)
	}
	return append(bytes.TrimRight(out.Bytes(), "\n"), '\n')
}

// words reports whether a goroutine state is the runtime's own wording:
// letters, digits, spaces and commas.
func words(value string) bool {
	if value == "" || len(value) > 64 {
		return false
	}
	for _, b := range []byte(value) {
		if !(b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || b == ' ' || b == ',') {
			return false
		}
	}
	return true
}

// trace reads a reduced trace: its class, when it has one, and the frames of
// the wanted goroutine, oldest first as a report lists them. A trace without
// that goroutine gives its first one.
func trace(reduced []byte, wanted string) (class string, frames *sentry.Stacktrace) {
	frames = &sentry.Stacktrace{Frames: []sentry.Frame{}}
	block, chosen := 0, false
	for line := range strings.SplitSeq(string(reduced), "\n") {
		if strings.HasPrefix(line, "goroutine") {
			if chosen {
				break
			}
			block++
			if line == wanted || strings.HasPrefix(line, wanted+" ") {
				frames.Frames, chosen = frames.Frames[:0], true
			}
			continue
		}
		if block == 0 {
			if line != "" {
				class = line
			}
			continue
		}
		if block > 1 && !chosen || line == "" || strings.HasPrefix(line, "created by ") {
			continue
		}
		at := strings.LastIndexByte(line, ':')
		if at < 0 || len(frames.Frames) >= keptFrames {
			continue
		}
		number, _ := strconv.Atoi(line[at+1:])
		frame := sentry.Frame{Function: line[:at], Lineno: number}
		// A function is "<import path>.<name>": the package ends at the
		// first dot after the last slash.
		if dot := strings.IndexByte(line[strings.LastIndexByte(line[:at], '/')+1:at], '.'); dot >= 0 {
			cut := strings.LastIndexByte(line[:at], '/') + 1 + dot
			frame.Module, frame.Function = line[:cut], line[cut+1:at]
		}
		frames.Frames = append(frames.Frames, frame)
	}
	for left, right := 0, len(frames.Frames)-1; left < right; left, right = left+1, right-1 {
		frames.Frames[left], frames.Frames[right] = frames.Frames[right], frames.Frames[left]
	}
	return class, frames
}

// report sends one interface failure while error reports are on, under the
// same hourly limit as every other report. kind is "hang" or "crash".
func (c *Control) report(kind string, reduced []byte) {
	if c == nil {
		return
	}
	wanted := "goroutine 1"
	class, frames := trace(reduced, wanted)
	if kind == "hang" {
		class = "unresponsive"
	} else if class == "" {
		class = "unknown"
	}
	c.mu.Lock()
	if !c.errorsLocked() || c.closing {
		c.mu.Unlock()
		return
	}
	event := c.eventLocked(kind, logging.Fields{"area": "interface", "operation": kind, "code": class}, frames)
	if !c.admitLocked(event.Fingerprint[0], time.Now()) || c.inFlight >= reportsInFlight {
		c.mu.Unlock()
		return
	}
	event.Attachments = []*sentry.Attachment{{Filename: "goroutines.txt", ContentType: "text/plain", Payload: reduced}}
	c.inFlight++
	c.reports.Add(1)
	scope, epoch := c.scope, c.epoch
	c.mu.Unlock()
	go func() {
		defer c.reports.Done()
		c.mu.Lock()
		allowed := c.errorsLocked() && c.epoch == epoch
		c.mu.Unlock()
		if allowed {
			c.send(scope, event)
		}
		c.mu.Lock()
		c.inFlight--
		c.mu.Unlock()
	}()
}

// Crash is the reduced trace of the fatal panic that ended the previous run.
type Crash struct{ reduced []byte }

// CatchCrashes makes the runtime copy a fatal panic's traceback into the
// application's log directory, besides printing it and exiting as before.
// MyGo offers no hook that recovers a panic on the interface thread or in an
// interface callback, so the process still ends; the next launch calls this
// again, which reduces the copy into CrashFile, removes the copy and returns
// the reduced trace for ReportCrash. It returns nil when the previous run
// did not crash.
func CatchCrashes(directory string) (*Crash, error) {
	output := filepath.Join(directory, crashOutput)
	var crash *Crash
	if raw, err := os.ReadFile(output); err == nil && len(raw) > 0 {
		crash = &Crash{reduced: reduce(raw)}
		if err := os.WriteFile(filepath.Join(directory, CrashFile), crash.reduced, 0o600); err != nil {
			return nil, err
		}
	}
	file, err := os.OpenFile(output, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return crash, err
	}
	defer file.Close()
	return crash, debug.SetCrashOutput(file, debug.CrashOptions{})
}

// ReportCrash sends the previous run's crash once, while error reports are
// on. While they are off the trace stays in the log directory only.
func (c *Control) ReportCrash(crash *Crash) {
	if crash != nil {
		c.report("crash", crash.reduced)
	}
}

// WatchdogOptions configures a Watchdog.
type WatchdogOptions struct {
	// Ping runs done on the interface thread without drawing anything. It
	// may block until done has run.
	Ping func(done func())
	// Directory is the application's log directory.
	Directory string
	// Control sends the report; nil sends nothing.
	Control *Control
	// Clock is the real clock by default.
	Clock clockwork.Clock
}

// Watchdog notices an interface thread that stops answering. While the
// interface draws frames, and for watchLinger after its last one, it asks
// the thread for a heartbeat every second. A heartbeat left unanswered for
// HangAfter while a window is visible and the system is awake is one hang:
// every goroutine's reduced trace is written to HangFile, and reported once
// while error reports are on. A second without the watchdog's own tick (the
// system slept, a debugger stopped the process) restarts the measurement
// instead. An idle interface has no heartbeat and no timer.
type Watchdog struct {
	options WatchdogOptions

	mu        sync.Mutex
	visible   bool
	suspended bool
	closed    bool
	running   bool
	touched   time.Time
	// pending is when the unanswered heartbeat was asked for, or when the
	// measurement last restarted.
	pending  time.Time
	waiting  bool
	reported bool
	done     sync.WaitGroup
}

// NewWatchdog returns a stopped watchdog; Touch starts it.
func NewWatchdog(options WatchdogOptions) *Watchdog {
	if options.Clock == nil {
		options.Clock = clockwork.NewRealClock()
	}
	return &Watchdog{options: options}
}

// Touch records interface activity: a frame was drawn.
func (w *Watchdog) Touch() {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.touched = w.options.Clock.Now()
	if w.running || w.closed {
		return
	}
	w.running = true
	w.done.Add(1)
	go w.watch()
}

// Visible records whether any window is on screen.
func (w *Watchdog) Visible(visible bool) {
	w.mu.Lock()
	w.visible = visible
	w.mu.Unlock()
}

// Suspended records whether the system is asleep.
func (w *Watchdog) Suspended(suspended bool) {
	w.mu.Lock()
	w.suspended = suspended
	w.mu.Unlock()
}

// Close stops the heartbeat.
func (w *Watchdog) Close() {
	w.mu.Lock()
	w.closed = true
	w.mu.Unlock()
	w.done.Wait()
}

func (w *Watchdog) watch() {
	defer w.done.Done()
	clock := w.options.Clock
	last := clock.Now()
	for {
		timer := clock.NewTimer(watchInterval)
		<-timer.Chan()
		now := clock.Now()
		stalled := now.Sub(last) >= 2*watchInterval
		last = now

		w.mu.Lock()
		if w.closed {
			w.running = false
			w.mu.Unlock()
			return
		}
		if stalled || w.suspended || !w.visible {
			// Not the interface's fault: measure again from here.
			w.pending = now
		}
		switch {
		case !w.waiting && now.Sub(w.touched) >= watchLinger:
			w.running = false
			w.mu.Unlock()
			return
		case !w.waiting:
			w.waiting, w.pending = true, now
			w.mu.Unlock()
			go w.options.Ping(func() {
				w.mu.Lock()
				w.waiting, w.reported = false, false
				w.mu.Unlock()
			})
		case !w.reported && now.Sub(w.pending) >= HangAfter:
			w.reported = true
			w.mu.Unlock()
			w.hang()
			// Writing the trace took time the interface is not charged for.
			last = clock.Now()
		default:
			w.mu.Unlock()
		}
	}
}

// hang keeps and reports every goroutine's reduced trace.
func (w *Watchdog) hang() {
	buffer := make([]byte, stackLimit)
	reduced := reduce(buffer[:runtime.Stack(buffer, true)])
	// A trace that cannot be kept is still reported.
	_ = os.WriteFile(filepath.Join(w.options.Directory, HangFile), reduced, 0o600)
	w.options.Control.report("hang", reduced)
}
