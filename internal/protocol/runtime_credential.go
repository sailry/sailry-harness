package protocol

// RuntimeCredential is the execution-only projection of an authorization.
// Renewal material belongs to the controller and has no field in this contract.
type RuntimeCredential struct {
	Controller           NodeID            `json:"controller"`
	Kind                 string            `json:"kind"`
	ID                   string            `json:"id"`
	Generation           string            `json:"generation"`
	Revision             uint64            `json:"revision"`
	State                string            `json:"state"`
	Authentication       Authentication    `json:"authentication,omitempty"`
	Provider             *ProviderID       `json:"provider,omitempty"`
	Package              *PluginReference  `json:"package,omitempty"`
	Flow                 string            `json:"flow,omitempty"`
	Endpoint             string            `json:"endpoint,omitempty"`
	Access               Secret            `json:"access"`
	IssuedAtMs           *uint64           `json:"issued_at_ms,omitempty"`
	ExpiresAtMs          *uint64           `json:"expires_at_ms,omitempty"`
	AccountID            *string           `json:"account_id,omitempty"`
	InstallationID       *string           `json:"installation_id,omitempty"`
	Scopes               []string          `json:"scopes,omitempty"`
	Headers              map[string]Secret `json:"headers,omitempty"`
	Slots                map[string]Secret `json:"slots,omitempty"`
	AccountIdentity      string            `json:"account_identity,omitempty"`
	AuthenticationTarget string            `json:"authentication_target,omitempty"`
}

func (value RuntimeCredential) CacheID() string { return value.Kind + ":" + value.ID }
