package acceptance

import "github.com/sailry/sailry-harness/internal/protocol"

// IDs refer to the original session acceptance checklist. Suffixes identify
// separate backend checks of one manual case; they do not claim UI coverage.
func backendCases() []backendCase {
	files := []string{"files"}
	commands := []string{"commands"}
	return []backendCase{
		{"A01", "Text and multi-turn context", true, nil, caseContext},
		{"A03", "Canonical Markdown text", false, nil, caseMarkdown},
		{"A05", "Canonical mathematical notation", false, nil, caseMath},
		{"A07", "Stop an active reply", false, nil, caseStop},
		{"A08", "Retry without mutating the original turn", false, nil, caseRetry},
		{"A09", "Replace a user message", false, nil, caseEdit},
		{"A10", "Fork at an earlier turn", false, nil, caseFork},
		{"A11", "Rewind with preserved backup history", false, nil, caseRewind},
		{"A13", "Search canonical conversation text", false, nil, caseConversationSearch},
		{"A15", "Unicode text round trip", false, nil, caseUnicode},
		{"D01", "List a fixture directory", false, files, caseList},
		{"D02", "Read an exact line range", true, files, caseRead},
		{"D03", "Find filenames and search file content", false, files, caseSearch},
		{"D04", "Create a file through the model tool", true, files, caseWrite},
		{"D05", "Edit a revisioned fixture file", false, files, caseFileEdit},
		{"D06", "Apply a patch across two files", false, files, casePatch},
		{"D09", "Reject a stale file revision", false, files, caseFileConflict},
		{"F01", "Command success with canonical output", false, commands, caseCommand},
		{"F01-exit7", "Preserve a command's raw exit 7", true, commands, caseCommandFailure},
		{"F01-exit127", "Preserve a command's raw exit 127", false, commands, caseCommandMissing},
		{"F02", "Start and observe a background command", false, commands, caseBackground},
		{"F03", "Write input and stop a background process", false, commands, caseCommandInput},
		{"F07", "Retain bounded large command output", false, commands, caseLargeCommand},
		{"I01", "Approve only the requested fixture write", true, files, caseApprove},
		{"I02-sandbox-ask", "Sandbox denial and one exact unrestricted retry", false, commands, caseSandboxAsk},
		{"I03-sandbox-full", "Full permission executes default and workspace requests unrestricted", false, commands, caseSandboxFull},
		{"I04", "Deny a fixture write without changing the file", false, files, caseDeny},
		{"J01", "Answer a concrete choice question", true, nil, caseQuestionChoice},
		{"J02", "Answer a yes-or-no question", false, nil, caseQuestionText},
		{"J05", "Decline a question and continue", false, nil, caseQuestionDecline},
		{"B02-plan", "Planning mode preserves read-only scope", false, files, casePlan},
		{"C01", "Deliver guidance at a model boundary", false, nil, caseGuidance},
		{"C02", "Edit, remove and send queued input", false, nil, caseQueue},
		{"P01", "Compact an empty conversation without a model reply", false, nil, caseCompactEmpty},
		{"P02", "Preserve context through manual compaction", false, nil, caseCompact},
	}
}

func (run *caseRun) standard() error {
	return run.newSession(protocol.WorkModeCode, protocol.PermissionProject)
}
