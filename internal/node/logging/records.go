package logging

import (
	"log/slog"
	"slices"
	"time"
)

// Phases of an observed operation.
const (
	Begin       = "begin"
	Admitted    = "admitted"
	Completed   = "completed"
	Failed      = "failed"
	Cancelled   = "cancelled"
	Interrupted = "interrupted"
)

// level is a record's severity when its caller names none: a failure is an
// error, an interrupted or unknown outcome a warning.
func level(phase, code string) slog.Level {
	switch {
	case phase == Failed:
		return slog.LevelError
	case phase == Interrupted || code == "outcome_unknown":
		return slog.LevelWarn
	}
	return slog.LevelInfo
}

// Span observes one operation. It never cancels or replays the work it
// describes.
type Span struct {
	attrs    []slog.Attr
	started  time.Time
	finished bool
	routine  bool
	debug    bool
}

// NewSpan records the operation's beginning.
func NewSpan(area string, attrs ...slog.Attr) *Span {
	span := &Span{attrs: append([]slog.Attr{slog.String("area", area)}, attrs...), started: time.Now()}
	span.emit(slog.LevelInfo, Begin, "")
	return span
}

// NewQuery observes an ordinary read: only a slow completion, a failure or
// an uncertain outcome is recorded.
func NewQuery(area string, attrs ...slog.Attr) *Span {
	return &Span{attrs: append([]slog.Attr{slog.String("area", area)}, attrs...), started: time.Now(), routine: true}
}

// NewBackground observes a package's read at debug level.
func NewBackground(area string, attrs ...slog.Attr) *Span {
	span := NewQuery(area, attrs...)
	span.routine, span.debug = false, true
	return span
}

// Set adds or replaces a field.
func (span *Span) Set(attr slog.Attr) {
	if index := slices.IndexFunc(span.attrs, func(existing slog.Attr) bool { return existing.Key == attr.Key }); index >= 0 {
		span.attrs[index] = attr
		return
	}
	span.attrs = append(span.attrs, attr)
}

func (span *Span) emit(severity slog.Level, phase, code string, extra ...slog.Attr) {
	attrs := append(slices.Clone(span.attrs), slog.String("phase", phase), slog.String("code", code))
	Emit(severity, append(attrs, extra...)...)
}

// Record notes an intermediate phase with the time since the beginning.
func (span *Span) Record(phase string) {
	if span.routine {
		return
	}
	severity := level(phase, "")
	if span.debug {
		severity = slog.LevelDebug
	}
	span.emit(severity, phase, "", slog.Uint64("duration_us", uint64(time.Since(span.started).Microseconds())))
}

// Finish records the outcome once. code is "" for success; warn lowers an
// expected failure to a warning.
func (span *Span) Finish(phase, code string, warn bool) {
	if span.finished {
		return
	}
	span.finished = true
	elapsed := time.Since(span.started)
	if span.routine && phase == Completed && elapsed < 100*time.Millisecond {
		return
	}
	severity := level(phase, code)
	if warn {
		severity = slog.LevelWarn
	} else if span.debug && phase == Completed && code == "" {
		severity = slog.LevelDebug
	}
	span.emit(severity, phase, code, slog.Uint64("duration_us", uint64(elapsed.Microseconds())))
}
