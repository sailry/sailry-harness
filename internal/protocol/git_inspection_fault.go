package protocol

import "encoding/json"

type GitInspectionReason string

const (
	GitInspectionWorktreeOpenFailed     GitInspectionReason = "worktree_open_failed"
	GitInspectionHeadReadFailed         GitInspectionReason = "head_read_failed"
	GitInspectionIndexReadFailed        GitInspectionReason = "index_read_failed"
	GitInspectionFilterConfigReadFailed GitInspectionReason = "filter_config_read_failed"
	GitInspectionStatusCommandFailed    GitInspectionReason = "status_command_failed"
	GitInspectionStatusNonzeroExit      GitInspectionReason = "status_nonzero_exit"
	GitInspectionIncompleteOutput       GitInspectionReason = "incomplete_output"
	GitInspectionUnsupportedEncoding    GitInspectionReason = "unsupported_path_encoding"
	GitInspectionInvalidRecord          GitInspectionReason = "invalid_record"
	GitInspectionUnsupportedStatus      GitInspectionReason = "unsupported_status_classification"
	GitInspectionIncompleteHeaders      GitInspectionReason = "incomplete_headers"
	GitInspectionIndexRecheckFailed     GitInspectionReason = "index_recheck_failed"
	GitInspectionHeadRecheckFailed      GitInspectionReason = "head_recheck_failed"
	GitInspectionIndexChanged           GitInspectionReason = "index_changed"
	GitInspectionHeadChanged            GitInspectionReason = "head_changed"
	GitInspectionDeadlineExceeded       GitInspectionReason = "deadline_exceeded"
)

// Stage is derived from the reason so the two diagnostic fields have one closed list.
func (reason GitInspectionReason) Stage() string {
	switch reason {
	case GitInspectionWorktreeOpenFailed:
		return "open_worktree"
	case GitInspectionHeadReadFailed:
		return "read_head"
	case GitInspectionIndexReadFailed:
		return "read_index"
	case GitInspectionFilterConfigReadFailed:
		return "read_filter_config"
	case GitInspectionStatusCommandFailed, GitInspectionStatusNonzeroExit:
		return "run_status"
	case GitInspectionIncompleteOutput, GitInspectionUnsupportedEncoding, GitInspectionInvalidRecord,
		GitInspectionUnsupportedStatus, GitInspectionIncompleteHeaders:
		return "parse_status"
	case GitInspectionIndexRecheckFailed, GitInspectionHeadRecheckFailed, GitInspectionIndexChanged, GitInspectionHeadChanged:
		return "verify_metadata"
	case GitInspectionDeadlineExceeded:
		return "read_deadline"
	}
	return ""
}

func (reason GitInspectionReason) MarshalJSON() ([]byte, error) { return marshalEnum(reason) }

func (reason *GitInspectionReason) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	if GitInspectionReason(decoded).Stage() == "" {
		return unknownEnum(data, reason, decoded)
	}
	*reason = GitInspectionReason(decoded)
	return nil
}
