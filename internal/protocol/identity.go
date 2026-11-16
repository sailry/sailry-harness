package protocol

import "encoding/json"

// InvitationKind is chosen by the authenticated device sharing an invitation.
// It is not a membership assertion supplied by the claimant.
type InvitationKind string

const (
	InvitationOtherUser InvitationKind = "other_user"
	InvitationMyDevice  InvitationKind = "my_device"
)

func (kind InvitationKind) Valid() bool {
	return kind == InvitationOtherUser || kind == InvitationMyDevice
}

func (kind InvitationKind) MarshalJSON() ([]byte, error) { return marshalEnum(kind) }
func (kind *InvitationKind) UnmarshalJSON(data []byte) error {
	var value string
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	if !InvitationKind(value).Valid() {
		return unknownEnum(data, kind, value)
	}
	*kind = InvitationKind(value)
	return nil
}

type UserDevice struct {
	Device    NodeID `json:"device"`
	User      UserID `json:"user"`
	Direction string `json:"direction"`
	MyDevice  bool   `json:"my_device"`
}

func (value *UserDevice) UnmarshalJSON(data []byte) error {
	type plain UserDevice
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"device", "user", "direction", "my_device"}, false); err != nil {
		return err
	}
	*value = UserDevice(decoded)
	return nil
}

// DirectoryHost carries an ordinary serialized Link address, like
// NetworkSettings.Address. Importing it grants outbound host access only.
type DirectoryHost struct {
	Node    NodeID `json:"node"`
	Name    string `json:"name"`
	Address string `json:"address"`
}

func (value *DirectoryHost) UnmarshalJSON(data []byte) error {
	type plain DirectoryHost
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"node", "name", "address"}, false); err != nil {
		return err
	}
	*value = DirectoryHost(decoded)
	return nil
}

type HostDirectory struct {
	Source   NodeID          `json:"source"`
	Revision uint64          `json:"revision"`
	Hosts    []DirectoryHost `json:"hosts"`
}

func (value *HostDirectory) UnmarshalJSON(data []byte) error {
	type plain HostDirectory
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"source", "revision", "hosts"}, false); err != nil {
		return err
	}
	*value = HostDirectory(decoded)
	return nil
}

type DeviceSponsorship struct {
	Sponsor NodeID    `json:"sponsor"`
	Request RequestID `json:"request"`
	Device  NodeID    `json:"device"`
	User    UserID    `json:"user"`
	State   string    `json:"state"`
}

func (value *DeviceSponsorship) UnmarshalJSON(data []byte) error {
	type plain DeviceSponsorship
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"sponsor", "request", "device", "user", "state"}, false); err != nil {
		return err
	}
	*value = DeviceSponsorship(decoded)
	return nil
}

type IdentityState struct {
	User         User                `json:"user"`
	Users        []User              `json:"users"`
	Devices      []UserDevice        `json:"devices"`
	Sponsorships []DeviceSponsorship `json:"sponsorships"`
}

func (value *IdentityState) UnmarshalJSON(data []byte) error {
	type plain IdentityState
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"user", "users", "devices", "sponsorships"}, false); err != nil {
		return err
	}
	*value = IdentityState(decoded)
	return nil
}

type SponsorDevice struct {
	Device     NodeID `json:"device"`
	Address    string `json:"address"`
	Generation uint64 `json:"generation"`
}

func (value *SponsorDevice) UnmarshalJSON(data []byte) error {
	type plain SponsorDevice
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"device", "address", "generation"}, false); err != nil {
		return err
	}
	*value = SponsorDevice(decoded)
	return nil
}

type ConfirmSponsorship struct {
	Sponsor NodeID    `json:"sponsor"`
	Request RequestID `json:"request"`
}

func (value *ConfirmSponsorship) UnmarshalJSON(data []byte) error {
	type plain ConfirmSponsorship
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"sponsor", "request"}, false); err != nil {
		return err
	}
	*value = ConfirmSponsorship(decoded)
	return nil
}

type RevokeDevice struct {
	Device      NodeID `json:"device"`
	ConfirmLast bool   `json:"confirm_last"`
	Sponsored   bool   `json:"sponsored"`
	Generation  uint64 `json:"generation"`
}

func (value *RevokeDevice) UnmarshalJSON(data []byte) error {
	type plain RevokeDevice
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"device"}, false); err != nil {
		return err
	}
	*value = RevokeDevice(decoded)
	return nil
}

type UserAdministrator struct {
	User          UserID `json:"user"`
	Administrator bool   `json:"administrator"`
}

func (value *UserAdministrator) UnmarshalJSON(data []byte) error {
	type plain UserAdministrator
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"user", "administrator"}, false); err != nil {
		return err
	}
	*value = UserAdministrator(decoded)
	return nil
}

type AssignOrphanUser struct {
	Source UserID `json:"source"`
	Target UserID `json:"target"`
}

func (value *AssignOrphanUser) UnmarshalJSON(data []byte) error {
	type plain AssignOrphanUser
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"source", "target"}, false); err != nil {
		return err
	}
	*value = AssignOrphanUser(decoded)
	return nil
}

type DeviceRevoked struct {
	Device NodeID `json:"device"`
}

func (value *DeviceRevoked) UnmarshalJSON(data []byte) error {
	type plain DeviceRevoked
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"device"}, false); err != nil {
		return err
	}
	*value = DeviceRevoked(decoded)
	return nil
}

type IdentityCommand struct {
	Kind string
	Data any
}

var identityCommandVariants = variantFactories{
	"read_identity":          nil,
	"read_host_directory":    nil,
	"sponsor_device":         func() any { return new(SponsorDevice) },
	"confirm_sponsorship":    func() any { return new(ConfirmSponsorship) },
	"revoke_device":          func() any { return new(RevokeDevice) },
	"set_user_administrator": func() any { return new(UserAdministrator) },
	"assign_orphan_user":     func() any { return new(AssignOrphanUser) },
}

func (value IdentityCommand) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, identityCommandVariants)
}
func (value *IdentityCommand) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *IdentityCommand) DecodeJSON(decoder *Decoder) error {
	kind, valueData, err := decoder.Union("kind", "data", identityCommandVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, valueData
	return nil
}

func (value IdentityCommand) ReadOnly() bool {
	return value.Kind == "read_identity" || value.Kind == "read_host_directory"
}

func init() {
	commandVariants["identity"] = func() any { return new(IdentityCommand) }
	outputVariants["identity_state"] = func() any { return new(IdentityState) }
	outputVariants["device_sponsorship"] = func() any { return new(DeviceSponsorship) }
	outputVariants["device_revoked"] = func() any { return new(DeviceRevoked) }
	outputVariants["user_changed"] = func() any { return new(User) }
	outputVariants["host_directory"] = func() any { return new(HostDirectory) }
}
