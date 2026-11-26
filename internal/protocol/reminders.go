package protocol

import "encoding/json"

// Reminder is the canonical personal reminder record persisted under the
// "reminders" storage namespace (`reminder/<uuid>`).
type Reminder struct {
	ID            string     `json:"id"`
	ExecutionNode *NodeID    `json:"execution_node"`
	Project       *ProjectID `json:"project"`
	Title         string     `json:"title"`
	Message       string     `json:"message"`
	Completed     bool       `json:"completed"`
	DueMs         *int64     `json:"due_ms"`
	NotifiedMs    *int64     `json:"notified_ms"`
}

// ReminderEntry pairs one reminder with its storage revision on its owning
// execution Node. Unavailable entries retain the last synced copy with a nil
// revision so they remain visible while read-only.
type ReminderEntry struct {
	Reminder    Reminder `json:"reminder"`
	Revision    *uint64  `json:"revision"`
	Unavailable bool     `json:"unavailable,omitempty"`
}

// ReminderPage is one cursor-bounded page of reminders.
type ReminderPage struct {
	Items []ReminderEntry `json:"items"`
	After *string         `json:"after"`
}

// ReminderRemoved reports the removal of one reminder.
type ReminderRemoved struct {
	ID      string `json:"id"`
	Removed bool   `json:"removed"`
}

// ReminderHistory returns delivery dispatch jobs for one reminder, newest first.
type ReminderHistory struct {
	Items []DispatchJob `json:"items"`
}

// ReminderDelivery is the outcome of a reminder notification dispatch callback.
type ReminderDelivery struct {
	Delivered bool `json:"delivered,omitempty"`
	Skipped   bool `json:"skipped,omitempty"`
}

type CommandListRemindersPayload struct {
	After     *string `json:"after,omitempty"`
	Completed *bool   `json:"completed,omitempty"`
}

type CommandReadReminderPayload struct {
	ID string `json:"id"`
}

type CommandSaveReminderPayload struct {
	Reminder      Reminder  `json:"reminder"`
	Revision      uint64    `json:"revision"`
	ExpectedValue *Reminder `json:"expected_value,omitempty"`
}

type CommandRemoveReminderPayload struct {
	ID            string    `json:"id"`
	Revision      uint64    `json:"revision"`
	ExpectedValue *Reminder `json:"expected_value,omitempty"`
}

type CommandReadReminderHistoryPayload struct {
	Handler string `json:"handler"`
}

// CommandDeliverReminderPayload carries the bound schedule payload when Node
// dispatch invokes native reminder delivery.
type CommandDeliverReminderPayload struct {
	Input json.RawMessage `json:"input"`
}
