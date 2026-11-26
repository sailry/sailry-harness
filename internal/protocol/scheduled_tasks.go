package protocol

// ScheduledTask is the canonical personal scheduled-task record persisted under
// the "scheduled-tasks" storage namespace (`task/<uuid>`).
type ScheduledTask struct {
	ID            string                `json:"id"`
	ExecutionNode *NodeID               `json:"execution_node"`
	Name          string                `json:"name"`
	Prompt        string                `json:"prompt"`
	Project       *ProjectID            `json:"project"`
	Worktree      *WorktreeID           `json:"worktree"`
	Config        SessionConfig         `json:"config"`
	Execution     ConversationExecution `json:"execution"`
	Team          *TeamSelection        `json:"team"`
	Models        map[RoleID]RoleModel  `json:"models"`
	Timing        DispatchTiming        `json:"timing"`
	Enabled       bool                  `json:"enabled"`
	Queue         string                `json:"queue"`
}

// ScheduledTaskEntry pairs one scheduled task with its owning execution Node's
// revision and next scheduled fire time. Unavailable entries keep the last
// synced copy with a nil revision so offline tasks remain visible.
type ScheduledTaskEntry struct {
	Task        ScheduledTask `json:"task"`
	Revision    *uint64       `json:"revision"`
	NextMs      *int64        `json:"next_ms"`
	Unavailable bool          `json:"unavailable,omitempty"`
}

// ScheduledTaskPage is one cursor-bounded page of scheduled tasks.
type ScheduledTaskPage struct {
	Items []ScheduledTaskEntry `json:"items"`
	After *string              `json:"after"`
}

// ScheduledTaskRunState summarizes the active or most recent dispatch job state
// for one scheduled task handler.
type ScheduledTaskRunState struct {
	Status    DispatchStatus `json:"status"`
	StartedMs *int64         `json:"started_ms"`
}

// ScheduledTaskRuntime bundles per-task job states and current dispatch
// schedules on the target execution Node.
type ScheduledTaskRuntime struct {
	Status    map[string]ScheduledTaskRunState `json:"status"`
	Schedules []DispatchSchedule               `json:"schedules"`
}

// ScheduledTaskHistoryEntry links a dispatch job to the conversation session
// and turn admitted by its callback, when available.
type ScheduledTaskHistoryEntry struct {
	Job           DispatchJob `json:"job"`
	ExecutionNode NodeID      `json:"execution_node"`
	Name          string      `json:"name"`
	Session       *SessionID  `json:"session"`
	Turn          *TurnID     `json:"turn"`
}

// ScheduledTaskHistory is one page of scheduled-task run records, newest first.
type ScheduledTaskHistory struct {
	Items      []ScheduledTaskHistoryEntry `json:"items"`
	NextBefore *uint64                     `json:"next_before"`
}

// ScheduledTaskRemoved reports removal of one scheduled task.
type ScheduledTaskRemoved struct {
	ID      string `json:"id"`
	Removed bool   `json:"removed"`
}

// ScheduledTaskStopped reports that a running scheduled-task turn was signalled
// to stop.
type ScheduledTaskStopped struct {
	ID      JobID `json:"id"`
	Stopped bool  `json:"stopped"`
}

type CommandListScheduledTasksPayload struct {
	After *string `json:"after,omitempty"`
}

type CommandReadScheduledTaskPayload struct {
	ID string `json:"id"`
}

type CommandSaveScheduledTaskPayload struct {
	Task          ScheduledTask  `json:"task"`
	Revision      uint64         `json:"revision"`
	ExpectedValue *ScheduledTask `json:"expected_value,omitempty"`
}

type CommandRemoveScheduledTaskPayload struct {
	ID            string         `json:"id"`
	Revision      uint64         `json:"revision"`
	ExpectedValue *ScheduledTask `json:"expected_value,omitempty"`
}

type CommandRunScheduledTaskPayload struct {
	ID            string         `json:"id"`
	Revision      *uint64        `json:"revision,omitempty"`
	ExpectedValue *ScheduledTask `json:"expected_value,omitempty"`
}

type CommandReadScheduledTaskHistoryPayload struct {
	Before        *uint64 `json:"before,omitempty"`
	Handler       *string `json:"handler,omitempty"`
	Limit         uint16  `json:"limit,omitempty"`
	ExecutionNode *NodeID `json:"execution_node,omitempty"`
}

type CommandCancelScheduledTaskJobPayload struct {
	ID            JobID   `json:"id"`
	ExecutionNode *NodeID `json:"execution_node,omitempty"`
}

type CommandStopScheduledTaskJobPayload struct {
	ID            JobID   `json:"id"`
	ExecutionNode *NodeID `json:"execution_node,omitempty"`
}
