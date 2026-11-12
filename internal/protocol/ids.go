package protocol

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/google/uuid"
)

type NodeID [32]byte

func (identity NodeID) String() string { return hex.EncodeToString(identity[:]) }

func (identity *NodeID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *NodeID) DecodeJSON(decoder *Decoder) error {
	var values []*uint8
	if err := decoder.Value(&values); err != nil {
		return err
	}
	if len(values) != len(identity) {
		return fmt.Errorf("node identity must contain %d bytes", len(identity))
	}
	var decoded NodeID
	for index, value := range values {
		if value == nil {
			return errors.New("node identity bytes must be integers")
		}
		decoded[index] = *value
	}
	*identity = decoded
	return nil
}

func parseIdentifier(value string) (uuid.UUID, error) {
	if len(value) == 38 && (value[0] != '{' || value[37] != '}') ||
		len(value) == 45 && value[:9] != "urn:uuid:" {
		return uuid.Nil, errors.New("invalid identifier")
	}
	identity, err := uuid.Parse(value)
	if err != nil {
		return uuid.Nil, errors.New("invalid identifier")
	}
	if identity == uuid.Nil {
		return uuid.Nil, errors.New("identifier must not be nil")
	}
	return identity, nil
}

func identifierJSON(identity [16]byte) ([]byte, error) {
	return json.Marshal(uuid.UUID(identity).String())
}

func identifierFrom(decoder *Decoder) (uuid.UUID, error) {
	value, err := decoder.String()
	if err != nil {
		return uuid.Nil, err
	}
	return parseIdentifier(value)
}

type RequestID [16]byte

func NewRequestID() RequestID { return RequestID(uuid.New()) }

func ParseRequestID(value string) (RequestID, error) {
	identity, err := parseIdentifier(value)
	return RequestID(identity), err
}

func (identity RequestID) String() string { return uuid.UUID(identity).String() }

func (identity RequestID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *RequestID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *RequestID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = RequestID(decoded)
	return nil
}

func (identity RequestID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *RequestID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = RequestID(decoded)
	return nil
}

type ProjectID [16]byte

func NewProjectID() ProjectID { return ProjectID(uuid.New()) }

func ParseProjectID(value string) (ProjectID, error) {
	identity, err := parseIdentifier(value)
	return ProjectID(identity), err
}

func (identity ProjectID) String() string { return uuid.UUID(identity).String() }

func (identity ProjectID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *ProjectID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *ProjectID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = ProjectID(decoded)
	return nil
}

func (identity ProjectID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *ProjectID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = ProjectID(decoded)
	return nil
}

type WorktreeID [16]byte

func NewWorktreeID() WorktreeID { return WorktreeID(uuid.New()) }

func ParseWorktreeID(value string) (WorktreeID, error) {
	identity, err := parseIdentifier(value)
	return WorktreeID(identity), err
}

func (identity WorktreeID) String() string { return uuid.UUID(identity).String() }

func (identity WorktreeID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *WorktreeID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *WorktreeID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = WorktreeID(decoded)
	return nil
}

func (identity WorktreeID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *WorktreeID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = WorktreeID(decoded)
	return nil
}

type SessionID [16]byte

func NewSessionID() SessionID { return SessionID(uuid.New()) }

func ParseSessionID(value string) (SessionID, error) {
	identity, err := parseIdentifier(value)
	return SessionID(identity), err
}

func (identity SessionID) String() string { return uuid.UUID(identity).String() }

func (identity SessionID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *SessionID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *SessionID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = SessionID(decoded)
	return nil
}

func (identity SessionID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *SessionID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = SessionID(decoded)
	return nil
}

type TurnID [16]byte

func NewTurnID() TurnID { return TurnID(uuid.New()) }

func ParseTurnID(value string) (TurnID, error) {
	identity, err := parseIdentifier(value)
	return TurnID(identity), err
}

func (identity TurnID) String() string { return uuid.UUID(identity).String() }

func (identity TurnID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *TurnID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *TurnID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = TurnID(decoded)
	return nil
}

func (identity TurnID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *TurnID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = TurnID(decoded)
	return nil
}

type ApprovalID [16]byte

func NewApprovalID() ApprovalID { return ApprovalID(uuid.New()) }

func ParseApprovalID(value string) (ApprovalID, error) {
	identity, err := parseIdentifier(value)
	return ApprovalID(identity), err
}

func (identity ApprovalID) String() string { return uuid.UUID(identity).String() }

func (identity ApprovalID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *ApprovalID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *ApprovalID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = ApprovalID(decoded)
	return nil
}

func (identity ApprovalID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *ApprovalID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = ApprovalID(decoded)
	return nil
}

type CheckpointID [16]byte

func NewCheckpointID() CheckpointID { return CheckpointID(uuid.New()) }

func ParseCheckpointID(value string) (CheckpointID, error) {
	identity, err := parseIdentifier(value)
	return CheckpointID(identity), err
}

func (identity CheckpointID) String() string { return uuid.UUID(identity).String() }

func (identity CheckpointID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *CheckpointID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *CheckpointID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = CheckpointID(decoded)
	return nil
}

func (identity CheckpointID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *CheckpointID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = CheckpointID(decoded)
	return nil
}

type QuestionID [16]byte

func NewQuestionID() QuestionID { return QuestionID(uuid.New()) }

func ParseQuestionID(value string) (QuestionID, error) {
	identity, err := parseIdentifier(value)
	return QuestionID(identity), err
}

func (identity QuestionID) String() string { return uuid.UUID(identity).String() }

func (identity QuestionID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *QuestionID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *QuestionID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = QuestionID(decoded)
	return nil
}

func (identity QuestionID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *QuestionID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = QuestionID(decoded)
	return nil
}

type TerminalID [16]byte

func NewTerminalID() TerminalID { return TerminalID(uuid.New()) }

func ParseTerminalID(value string) (TerminalID, error) {
	identity, err := parseIdentifier(value)
	return TerminalID(identity), err
}

func (identity TerminalID) String() string { return uuid.UUID(identity).String() }

func (identity TerminalID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *TerminalID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *TerminalID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = TerminalID(decoded)
	return nil
}

func (identity TerminalID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *TerminalID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = TerminalID(decoded)
	return nil
}

type StreamID [16]byte

func NewStreamID() StreamID { return StreamID(uuid.New()) }

func ParseStreamID(value string) (StreamID, error) {
	identity, err := parseIdentifier(value)
	return StreamID(identity), err
}

func (identity StreamID) String() string { return uuid.UUID(identity).String() }

func (identity StreamID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *StreamID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *StreamID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = StreamID(decoded)
	return nil
}

func (identity StreamID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *StreamID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = StreamID(decoded)
	return nil
}

type AttachmentID [16]byte

func NewAttachmentID() AttachmentID { return AttachmentID(uuid.New()) }

func ParseAttachmentID(value string) (AttachmentID, error) {
	identity, err := parseIdentifier(value)
	return AttachmentID(identity), err
}

func (identity AttachmentID) String() string { return uuid.UUID(identity).String() }

func (identity AttachmentID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *AttachmentID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *AttachmentID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = AttachmentID(decoded)
	return nil
}

func (identity AttachmentID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *AttachmentID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = AttachmentID(decoded)
	return nil
}

type ArtifactID [16]byte

func NewArtifactID() ArtifactID { return ArtifactID(uuid.New()) }

func (identity ArtifactID) String() string { return uuid.UUID(identity).String() }

func (identity ArtifactID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *ArtifactID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *ArtifactID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = ArtifactID(decoded)
	return nil
}

func (identity ArtifactID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *ArtifactID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = ArtifactID(decoded)
	return nil
}

type CredentialID [16]byte

func NewCredentialID() CredentialID { return CredentialID(uuid.New()) }

func (identity CredentialID) String() string { return uuid.UUID(identity).String() }

func (identity CredentialID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *CredentialID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *CredentialID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = CredentialID(decoded)
	return nil
}

func (identity CredentialID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *CredentialID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = CredentialID(decoded)
	return nil
}

type RoleID [16]byte

func NewRoleID() RoleID { return RoleID(uuid.New()) }

func ParseRoleID(value string) (RoleID, error) {
	identity, err := parseIdentifier(value)
	return RoleID(identity), err
}

func (identity RoleID) String() string { return uuid.UUID(identity).String() }

func (identity RoleID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *RoleID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *RoleID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = RoleID(decoded)
	return nil
}

func (identity RoleID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *RoleID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = RoleID(decoded)
	return nil
}

type TeamID [16]byte

func NewTeamID() TeamID { return TeamID(uuid.New()) }

func ParseTeamID(value string) (TeamID, error) {
	identity, err := parseIdentifier(value)
	return TeamID(identity), err
}

func (identity TeamID) String() string { return uuid.UUID(identity).String() }

func (identity TeamID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *TeamID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *TeamID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = TeamID(decoded)
	return nil
}

func (identity TeamID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *TeamID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = TeamID(decoded)
	return nil
}

type SshID [16]byte

func NewSshID() SshID { return SshID(uuid.New()) }

func ParseSshID(value string) (SshID, error) {
	identity, err := parseIdentifier(value)
	return SshID(identity), err
}

func (identity SshID) String() string { return uuid.UUID(identity).String() }

func (identity SshID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *SshID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *SshID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = SshID(decoded)
	return nil
}

func (identity SshID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *SshID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = SshID(decoded)
	return nil
}

type DatabaseID [16]byte

func NewDatabaseID() DatabaseID { return DatabaseID(uuid.New()) }

func ParseDatabaseID(value string) (DatabaseID, error) {
	identity, err := parseIdentifier(value)
	return DatabaseID(identity), err
}

func (identity DatabaseID) String() string { return uuid.UUID(identity).String() }

func (identity DatabaseID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *DatabaseID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *DatabaseID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = DatabaseID(decoded)
	return nil
}

func (identity DatabaseID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *DatabaseID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = DatabaseID(decoded)
	return nil
}

type ProviderID [16]byte

func NewProviderID() ProviderID { return ProviderID(uuid.New()) }

func ParseProviderID(value string) (ProviderID, error) {
	identity, err := parseIdentifier(value)
	return ProviderID(identity), err
}

func (identity ProviderID) String() string { return uuid.UUID(identity).String() }

func (identity ProviderID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *ProviderID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *ProviderID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = ProviderID(decoded)
	return nil
}

func (identity ProviderID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *ProviderID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = ProviderID(decoded)
	return nil
}

type EventID [16]byte

func NewEventID() EventID { return EventID(uuid.New()) }

func (identity EventID) String() string { return uuid.UUID(identity).String() }

func (identity EventID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *EventID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *EventID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = EventID(decoded)
	return nil
}

func (identity EventID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *EventID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = EventID(decoded)
	return nil
}

type JobID [16]byte

func NewJobID() JobID { return JobID(uuid.New()) }

func (identity JobID) String() string { return uuid.UUID(identity).String() }

func (identity JobID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *JobID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *JobID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = JobID(decoded)
	return nil
}

func (identity JobID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *JobID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = JobID(decoded)
	return nil
}

type KnowledgeID [16]byte

func NewKnowledgeID() KnowledgeID { return KnowledgeID(uuid.New()) }

func ParseKnowledgeID(value string) (KnowledgeID, error) {
	identity, err := parseIdentifier(value)
	return KnowledgeID(identity), err
}

func (identity KnowledgeID) String() string { return uuid.UUID(identity).String() }

func (identity KnowledgeID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *KnowledgeID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *KnowledgeID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = KnowledgeID(decoded)
	return nil
}

func (identity KnowledgeID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *KnowledgeID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = KnowledgeID(decoded)
	return nil
}

type ModelID [16]byte

func NewModelID() ModelID { return ModelID(uuid.New()) }

func ParseModelID(value string) (ModelID, error) {
	identity, err := parseIdentifier(value)
	return ModelID(identity), err
}

func (identity ModelID) String() string { return uuid.UUID(identity).String() }

func (identity ModelID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *ModelID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *ModelID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = ModelID(decoded)
	return nil
}

func (identity ModelID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *ModelID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = ModelID(decoded)
	return nil
}

type NotificationID [16]byte

func NewNotificationID() NotificationID { return NotificationID(uuid.New()) }

func (identity NotificationID) String() string { return uuid.UUID(identity).String() }

func (identity NotificationID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *NotificationID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *NotificationID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = NotificationID(decoded)
	return nil
}

func (identity NotificationID) MarshalText() ([]byte, error) {
	return uuid.UUID(identity).MarshalText()
}

func (identity *NotificationID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = NotificationID(decoded)
	return nil
}

type ScheduleID [16]byte

func NewScheduleID() ScheduleID { return ScheduleID(uuid.New()) }

func ParseScheduleID(value string) (ScheduleID, error) {
	identity, err := parseIdentifier(value)
	return ScheduleID(identity), err
}

func (identity ScheduleID) String() string { return uuid.UUID(identity).String() }

func (identity ScheduleID) MarshalJSON() ([]byte, error) { return identifierJSON(identity) }

func (identity *ScheduleID) UnmarshalJSON(data []byte) error { return DecodeValue(data, identity) }

func (identity *ScheduleID) DecodeJSON(decoder *Decoder) error {
	decoded, err := identifierFrom(decoder)
	if err != nil {
		return err
	}
	*identity = ScheduleID(decoded)
	return nil
}

func (identity ScheduleID) MarshalText() ([]byte, error) { return uuid.UUID(identity).MarshalText() }

func (identity *ScheduleID) UnmarshalText(data []byte) error {
	decoded, err := parseIdentifier(string(data))
	if err != nil {
		return err
	}
	*identity = ScheduleID(decoded)
	return nil
}
