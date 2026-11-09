// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type DispatchSource struct {
	Package string `json:"package"`
	Topic   string `json:"topic"`
}

func DefaultDispatchSource() DispatchSource {
	return DispatchSource{
		Package: "",
		Topic:   "",
	}
}
func (value *DispatchSource) UnmarshalJSON(data []byte) error {
	type plain DispatchSource
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "topic"}, true); err != nil {
		return err
	}
	*value = DispatchSource(decoded)
	return nil
}
func (value DispatchSource) MarshalJSON() ([]byte, error) {
	type plain DispatchSource
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchEvent struct {
	ID          EventID         `json:"id"`
	Source      DispatchSource  `json:"source"`
	Payload     json.RawMessage `json:"payload"`
	TimestampMs int64           `json:"timestamp_ms"`
	Schedule    *ScheduleID     `json:"schedule"`
	ScheduledMs *int64          `json:"scheduled_ms"`
}

func DefaultDispatchEvent() DispatchEvent {
	return DispatchEvent{
		ID:          EventID{},
		Source:      DefaultDispatchSource(),
		Payload:     json.RawMessage("null"),
		TimestampMs: 0,
		Schedule:    nil,
		ScheduledMs: nil,
	}
}
func (value *DispatchEvent) UnmarshalJSON(data []byte) error {
	type plain DispatchEvent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "source", "payload", "timestamp_ms"}, false); err != nil {
		return err
	}
	*value = DispatchEvent(decoded)
	return nil
}
func (value DispatchEvent) MarshalJSON() ([]byte, error) {
	type plain DispatchEvent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchScope struct {
	Worktree *WorktreeID `json:"worktree"`
	Session  *SessionID  `json:"session"`
}

func DefaultDispatchScope() DispatchScope {
	return DispatchScope{
		Worktree: nil,
		Session:  nil,
	}
}
func (value *DispatchScope) UnmarshalJSON(data []byte) error {
	type plain DispatchScope
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = DispatchScope(decoded)
	return nil
}
func (value DispatchScope) MarshalJSON() ([]byte, error) {
	type plain DispatchScope
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCallback struct {
	Scope      DispatchScope      `json:"scope"`
	Completion DispatchCompletion `json:"completion"`
	Command    *Command           `json:"command"`
	Bindings   map[string]string  `json:"bindings"`
}

func DefaultDispatchCallback() DispatchCallback {
	return DispatchCallback{
		Scope:      DefaultDispatchScope(),
		Completion: DispatchCompletion(""),
		Command:    nil,
		Bindings:   map[string]string{},
	}
}
func (value *DispatchCallback) UnmarshalJSON(data []byte) error {
	type plain DispatchCallback
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"scope", "completion", "command", "bindings"}, true); err != nil {
		return err
	}
	*value = DispatchCallback(decoded)
	return nil
}
func (value DispatchCallback) MarshalJSON() ([]byte, error) {
	type plain DispatchCallback
	encoded := plain(value)
	encoded.Bindings = normalizeCollections(encoded.Bindings)
	return json.Marshal(encoded)
}

type DispatchCompletion string

const (
	DispatchCompletionCommand DispatchCompletion = "command"
	DispatchCompletionTurn    DispatchCompletion = "turn"
)

func (value DispatchCompletion) MarshalJSON() ([]byte, error) {
	switch value {
	case DispatchCompletionCommand, DispatchCompletionTurn:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown DispatchCompletion %q", value)
	}
}
func (value *DispatchCompletion) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch DispatchCompletion(decoded) {
	case DispatchCompletionCommand, DispatchCompletionTurn:
		*value = DispatchCompletion(decoded)
		return nil
	default:
		return fmt.Errorf("unknown DispatchCompletion %q", decoded)
	}
}
func (value DispatchCompletion) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *DispatchCompletion) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type DispatchHandler struct {
	Name     string           `json:"name"`
	Revision uint64           `json:"revision"`
	Enabled  bool             `json:"enabled"`
	Source   DispatchSource   `json:"source"`
	Queue    string           `json:"queue"`
	Callback DispatchCallback `json:"callback"`
}

func DefaultDispatchHandler() DispatchHandler {
	return DispatchHandler{
		Name:     "",
		Revision: 0,
		Enabled:  false,
		Source:   DefaultDispatchSource(),
		Queue:    "",
		Callback: DefaultDispatchCallback(),
	}
}
func (value *DispatchHandler) UnmarshalJSON(data []byte) error {
	type plain DispatchHandler
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "revision", "enabled", "source", "queue", "callback"}, true); err != nil {
		return err
	}
	*value = DispatchHandler(decoded)
	return nil
}
func (value DispatchHandler) MarshalJSON() ([]byte, error) {
	type plain DispatchHandler
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchTiming struct {
	Kind string
	Data any
}
type DispatchTimingOncePayload struct {
	AtMs int64 `json:"at_ms"`
}

func DefaultDispatchTimingOncePayload() DispatchTimingOncePayload {
	return DispatchTimingOncePayload{
		AtMs: 0,
	}
}
func (value *DispatchTimingOncePayload) UnmarshalJSON(data []byte) error {
	type plain DispatchTimingOncePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"at_ms"}, true); err != nil {
		return err
	}
	*value = DispatchTimingOncePayload(decoded)
	return nil
}
func (value DispatchTimingOncePayload) MarshalJSON() ([]byte, error) {
	type plain DispatchTimingOncePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchTimingEveryPayload struct {
	AnchorMs   int64  `json:"anchor_ms"`
	IntervalMs uint64 `json:"interval_ms"`
}

func DefaultDispatchTimingEveryPayload() DispatchTimingEveryPayload {
	return DispatchTimingEveryPayload{
		AnchorMs:   0,
		IntervalMs: 0,
	}
}
func (value *DispatchTimingEveryPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchTimingEveryPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"anchor_ms", "interval_ms"}, true); err != nil {
		return err
	}
	*value = DispatchTimingEveryPayload(decoded)
	return nil
}
func (value DispatchTimingEveryPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchTimingEveryPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchTimingWeeklyPayload struct {
	Timezone string `json:"timezone"`
	Weekdays Bytes  `json:"weekdays"`
	Hour     uint8  `json:"hour"`
	Minute   uint8  `json:"minute"`
}

func DefaultDispatchTimingWeeklyPayload() DispatchTimingWeeklyPayload {
	return DispatchTimingWeeklyPayload{
		Timezone: "",
		Weekdays: Bytes{},
		Hour:     0,
		Minute:   0,
	}
}
func (value *DispatchTimingWeeklyPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchTimingWeeklyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"timezone", "weekdays", "hour", "minute"}, true); err != nil {
		return err
	}
	*value = DispatchTimingWeeklyPayload(decoded)
	return nil
}
func (value DispatchTimingWeeklyPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchTimingWeeklyPayload
	encoded := plain(value)
	encoded.Weekdays = normalizeCollections(encoded.Weekdays)
	return json.Marshal(encoded)
}

var dispatchTimingVariants = variantFactories{"once": func() any { return new(DispatchTimingOncePayload) },
	"every":  func() any { return new(DispatchTimingEveryPayload) },
	"weekly": func() any { return new(DispatchTimingWeeklyPayload) }}

func (value DispatchTiming) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, dispatchTimingVariants)
}
func (value *DispatchTiming) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", dispatchTimingVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DispatchSchedule struct {
	ID       ScheduleID      `json:"id"`
	Revision uint64          `json:"revision"`
	Enabled  bool            `json:"enabled"`
	Topic    string          `json:"topic"`
	Payload  json.RawMessage `json:"payload"`
	Timing   DispatchTiming  `json:"timing"`
	NextMs   *int64          `json:"next_ms"`
}

func DefaultDispatchSchedule() DispatchSchedule {
	return DispatchSchedule{
		ID:       ScheduleID{},
		Revision: 0,
		Enabled:  false,
		Topic:    "",
		Payload:  json.RawMessage("null"),
		Timing:   DispatchTiming{},
		NextMs:   nil,
	}
}
func (value *DispatchSchedule) UnmarshalJSON(data []byte) error {
	type plain DispatchSchedule
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "revision", "enabled", "topic", "payload", "timing"}, true); err != nil {
		return err
	}
	*value = DispatchSchedule(decoded)
	return nil
}
func (value DispatchSchedule) MarshalJSON() ([]byte, error) {
	type plain DispatchSchedule
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchStatus string

const (
	DispatchStatusQueued    DispatchStatus = "queued"
	DispatchStatusRunning   DispatchStatus = "running"
	DispatchStatusCompleted DispatchStatus = "completed"
	DispatchStatusFailed    DispatchStatus = "failed"
	DispatchStatusCancelled DispatchStatus = "cancelled"
	DispatchStatusUnknown   DispatchStatus = "unknown"
)

func (value DispatchStatus) MarshalJSON() ([]byte, error) {
	switch value {
	case DispatchStatusQueued, DispatchStatusRunning, DispatchStatusCompleted, DispatchStatusFailed, DispatchStatusCancelled, DispatchStatusUnknown:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown DispatchStatus %q", value)
	}
}
func (value *DispatchStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch DispatchStatus(decoded) {
	case DispatchStatusQueued, DispatchStatusRunning, DispatchStatusCompleted, DispatchStatusFailed, DispatchStatusCancelled, DispatchStatusUnknown:
		*value = DispatchStatus(decoded)
		return nil
	default:
		return fmt.Errorf("unknown DispatchStatus %q", decoded)
	}
}
func (value DispatchStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *DispatchStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type DispatchJob struct {
	ID         JobID          `json:"id"`
	Event      DispatchEvent  `json:"event"`
	Handler    string         `json:"handler"`
	Queue      string         `json:"queue"`
	Request    RequestID      `json:"request"`
	Status     DispatchStatus `json:"status"`
	CreatedMs  int64          `json:"created_ms"`
	StartedMs  *int64         `json:"started_ms"`
	FinishedMs *int64         `json:"finished_ms"`
	Error      *Fault         `json:"error"`
}

func DefaultDispatchJob() DispatchJob {
	return DispatchJob{
		ID:         JobID{},
		Event:      DefaultDispatchEvent(),
		Handler:    "",
		Queue:      "",
		Request:    RequestID{},
		Status:     DispatchStatus(""),
		CreatedMs:  0,
		StartedMs:  nil,
		FinishedMs: nil,
		Error:      nil,
	}
}
func (value *DispatchJob) UnmarshalJSON(data []byte) error {
	type plain DispatchJob
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "event", "handler", "queue", "request", "status", "created_ms"}, false); err != nil {
		return err
	}
	*value = DispatchJob(decoded)
	return nil
}
func (value DispatchJob) MarshalJSON() ([]byte, error) {
	type plain DispatchJob
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommand struct {
	Kind string
	Data any
}
type DispatchCommandRemoveHandlerPayload struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
	CancelPending    bool   `json:"cancel_pending"`
}

func DefaultDispatchCommandRemoveHandlerPayload() DispatchCommandRemoveHandlerPayload {
	return DispatchCommandRemoveHandlerPayload{
		Name:             "",
		ExpectedRevision: 0,
		CancelPending:    false,
	}
}
func (value *DispatchCommandRemoveHandlerPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandRemoveHandlerPayload
	decoded := plain{}
	decoded.CancelPending = false
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision"}, true); err != nil {
		return err
	}
	*value = DispatchCommandRemoveHandlerPayload(decoded)
	return nil
}
func (value DispatchCommandRemoveHandlerPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandRemoveHandlerPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandRemoveSchedulePayload struct {
	ID               ScheduleID `json:"id"`
	ExpectedRevision uint64     `json:"expected_revision"`
}

func DefaultDispatchCommandRemoveSchedulePayload() DispatchCommandRemoveSchedulePayload {
	return DispatchCommandRemoveSchedulePayload{
		ID:               ScheduleID{},
		ExpectedRevision: 0,
	}
}
func (value *DispatchCommandRemoveSchedulePayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandRemoveSchedulePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "expected_revision"}, true); err != nil {
		return err
	}
	*value = DispatchCommandRemoveSchedulePayload(decoded)
	return nil
}
func (value DispatchCommandRemoveSchedulePayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandRemoveSchedulePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandPublishPayload struct {
	Topic   string          `json:"topic"`
	Payload json.RawMessage `json:"payload"`
}

func DefaultDispatchCommandPublishPayload() DispatchCommandPublishPayload {
	return DispatchCommandPublishPayload{
		Topic:   "",
		Payload: json.RawMessage("null"),
	}
}
func (value *DispatchCommandPublishPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandPublishPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"topic", "payload"}, true); err != nil {
		return err
	}
	*value = DispatchCommandPublishPayload(decoded)
	return nil
}
func (value DispatchCommandPublishPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandPublishPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandEnqueuePayload struct {
	Handler string          `json:"handler"`
	Payload json.RawMessage `json:"payload"`
}

func DefaultDispatchCommandEnqueuePayload() DispatchCommandEnqueuePayload {
	return DispatchCommandEnqueuePayload{
		Handler: "",
		Payload: json.RawMessage("null"),
	}
}
func (value *DispatchCommandEnqueuePayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandEnqueuePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"handler", "payload"}, true); err != nil {
		return err
	}
	*value = DispatchCommandEnqueuePayload(decoded)
	return nil
}
func (value DispatchCommandEnqueuePayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandEnqueuePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandListJobsPayload struct {
	Before *uint64 `json:"before"`
	Limit  uint16  `json:"limit"`
}

func DefaultDispatchCommandListJobsPayload() DispatchCommandListJobsPayload {
	return DispatchCommandListJobsPayload{
		Before: nil,
		Limit:  0,
	}
}
func (value *DispatchCommandListJobsPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandListJobsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"limit"}, true); err != nil {
		return err
	}
	*value = DispatchCommandListJobsPayload(decoded)
	return nil
}
func (value DispatchCommandListJobsPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandListJobsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandReadJobPayload struct {
	ID JobID `json:"id"`
}

func DefaultDispatchCommandReadJobPayload() DispatchCommandReadJobPayload {
	return DispatchCommandReadJobPayload{
		ID: JobID{},
	}
}
func (value *DispatchCommandReadJobPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandReadJobPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, true); err != nil {
		return err
	}
	*value = DispatchCommandReadJobPayload(decoded)
	return nil
}
func (value DispatchCommandReadJobPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandReadJobPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandReadResultPayload struct {
	ID JobID `json:"id"`
}

func DefaultDispatchCommandReadResultPayload() DispatchCommandReadResultPayload {
	return DispatchCommandReadResultPayload{
		ID: JobID{},
	}
}
func (value *DispatchCommandReadResultPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandReadResultPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, true); err != nil {
		return err
	}
	*value = DispatchCommandReadResultPayload(decoded)
	return nil
}
func (value DispatchCommandReadResultPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandReadResultPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DispatchCommandCancelJobPayload struct {
	ID JobID `json:"id"`
}

func DefaultDispatchCommandCancelJobPayload() DispatchCommandCancelJobPayload {
	return DispatchCommandCancelJobPayload{
		ID: JobID{},
	}
}
func (value *DispatchCommandCancelJobPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchCommandCancelJobPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id"}, true); err != nil {
		return err
	}
	*value = DispatchCommandCancelJobPayload(decoded)
	return nil
}
func (value DispatchCommandCancelJobPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchCommandCancelJobPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var dispatchCommandVariants = variantFactories{"list_handlers": nil,
	"save_handler":    func() any { return new(DispatchHandler) },
	"remove_handler":  func() any { return new(DispatchCommandRemoveHandlerPayload) },
	"list_schedules":  nil,
	"save_schedule":   func() any { return new(DispatchSchedule) },
	"remove_schedule": func() any { return new(DispatchCommandRemoveSchedulePayload) },
	"publish":         func() any { return new(DispatchCommandPublishPayload) },
	"enqueue":         func() any { return new(DispatchCommandEnqueuePayload) },
	"list_jobs":       func() any { return new(DispatchCommandListJobsPayload) },
	"read_job":        func() any { return new(DispatchCommandReadJobPayload) },
	"read_result":     func() any { return new(DispatchCommandReadResultPayload) },
	"cancel_job":      func() any { return new(DispatchCommandCancelJobPayload) }}

func (value DispatchCommand) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, dispatchCommandVariants)
}
func (value *DispatchCommand) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", dispatchCommandVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DispatchPage struct {
	Jobs       []DispatchJob `json:"jobs"`
	NextBefore *uint64       `json:"next_before"`
}

func DefaultDispatchPage() DispatchPage {
	return DispatchPage{
		Jobs:       []DispatchJob{},
		NextBefore: nil,
	}
}
func (value *DispatchPage) UnmarshalJSON(data []byte) error {
	type plain DispatchPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"jobs"}, false); err != nil {
		return err
	}
	*value = DispatchPage(decoded)
	return nil
}
func (value DispatchPage) MarshalJSON() ([]byte, error) {
	type plain DispatchPage
	encoded := plain(value)
	encoded.Jobs = normalizeCollections(encoded.Jobs)
	return json.Marshal(encoded)
}

type DispatchOutput struct {
	Kind string
	Data any
}
type DispatchOutputPublishedPayload struct {
	Event EventID `json:"event"`
	Jobs  []JobID `json:"jobs"`
}

func DefaultDispatchOutputPublishedPayload() DispatchOutputPublishedPayload {
	return DispatchOutputPublishedPayload{
		Event: EventID{},
		Jobs:  []JobID{},
	}
}
func (value *DispatchOutputPublishedPayload) UnmarshalJSON(data []byte) error {
	type plain DispatchOutputPublishedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"event", "jobs"}, false); err != nil {
		return err
	}
	*value = DispatchOutputPublishedPayload(decoded)
	return nil
}
func (value DispatchOutputPublishedPayload) MarshalJSON() ([]byte, error) {
	type plain DispatchOutputPublishedPayload
	encoded := plain(value)
	encoded.Jobs = normalizeCollections(encoded.Jobs)
	return json.Marshal(encoded)
}

var dispatchOutputVariants = variantFactories{"handlers": func() any { return new([]DispatchHandler) },
	"handler":   func() any { return new(DispatchHandler) },
	"schedules": func() any { return new([]DispatchSchedule) },
	"schedule":  func() any { return new(DispatchSchedule) },
	"published": func() any { return new(DispatchOutputPublishedPayload) },
	"job":       func() any { return new(DispatchJob) },
	"jobs":      func() any { return new(DispatchPage) },
	"result":    func() any { return new(RequestOutcome) },
	"removed":   nil}

func (value DispatchOutput) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, dispatchOutputVariants)
}
func (value *DispatchOutput) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", dispatchOutputVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
