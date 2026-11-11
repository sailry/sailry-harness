package viewmodel

type EnvVar struct{ Key, Value string }

type ConnState int

const (
	Unconnected ConnState = iota
	Connecting
	Connected
	Expired
	NeedsConfirmation
	ConnFailed
)

type ServerState int

const (
	ServerUp ServerState = iota
	ServerStarting
	ServerDown
	ServerAuth
	// ServerIdle is a server that may connect and has not been asked to.
	ServerIdle
)

type ConnectorServer struct {
	Name      string
	Transport int
	Command   string
	Args      []string
	URL       string
	Env       []EnvVar
	Allowed   bool
	State     ServerState
	Tools     int
	Updated   string
	Error     string
	// Problem is the i18n key of what keeps the server from connecting, ""
	// when it may connect; Error then holds the reason, when one is known.
	Problem string
}

type ConnectorField struct {
	Name, Title string
	Secret      bool
	Help        bool
	Optional    bool
}

type ConnectorType struct {
	ID, Package, Name, Description string
	Methods                        []string
	Fields                         []ConnectorField
	Enabled                        bool
	// Icon is the icon the connector declares, as Package.Icon.
	Icon string
}

type ConnectorService struct {
	Revision                    uint64
	ID, Type, Name, Description string
	Identity                    string
	Method                      string
	Account                     bool
	State                       ConnState
	AccountOn                   bool
	Manual                      bool
	Servers                     []ConnectorServer
	Fields                      map[string]string
	// Icon is the icon its connector or package declares, as Package.Icon.
	Icon string
}
