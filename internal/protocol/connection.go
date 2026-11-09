// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

type ConnectionResource struct {
	Kind string
	Data any
}

var connectionResourceVariants = variantFactories{"database": func() any { return new(DatabaseID) },
	"ssh": func() any { return new(SshID) }}

func (value ConnectionResource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "id", value.Kind, value.Data, connectionResourceVariants)
}
func (value *ConnectionResource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "id", connectionResourceVariants, false)
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
