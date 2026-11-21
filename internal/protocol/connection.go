// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import "slices"

type ConnectionResource struct {
	Kind string
	Data any
}

var connectionResourceVariants = variantFactories{"database": func() any { return new(DatabaseID) },
	"ssh": func() any { return new(SshID) }}

func (value ConnectionResource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "id", value.Kind, value.Data, connectionResourceVariants)
}
func (value *ConnectionResource) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ConnectionResource) DecodeJSON(decoder *Decoder) error {
	kind, decoded, err := decoder.Union("kind", "id", connectionResourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ConnectionSharing struct {
	Kind string
	Data any
}

// ConnectionSharingPermits checks the explicit sharing of an unbound connection.
func ConnectionSharingPermits(project *ProjectID, sharing *ConnectionSharing) bool {
	if sharing == nil {
		return false
	}
	switch sharing.Kind {
	case "global":
		return true
	case "projects":
		if project != nil && *project != (ProjectID{}) {
			switch projects := sharing.Data.(type) {
			case []ProjectID:
				return slices.Contains(projects, *project)
			case *[]ProjectID:
				return projects != nil && slices.Contains(*projects, *project)
			}
		}
	}
	return false
}

var connectionSharingVariants = variantFactories{"global": nil,
	"projects": func() any { return new([]ProjectID) }}

func (value ConnectionSharing) MarshalJSON() ([]byte, error) {
	return MarshalUnion("scope", "projects", value.Kind, value.Data, connectionSharingVariants)
}
func (value *ConnectionSharing) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "scope", "projects", connectionSharingVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
