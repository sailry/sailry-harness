package preview

// The AI settings' preview data: providers, connectors, teams, plugins,
// skills, the market, MCP servers and the model library, as a Node would
// report them. Credentials are never part of it.

// Auth is how a provider channel signs in (list/summary.rs AuthKind).
type Auth int

const (
	AuthKey    Auth = iota // an API key
	AuthOAuth              // an account login
	AuthPlugin             // a plugin's own sign-in
	AuthNone               // a local endpoint
)

// ChannelModel is a model a channel offers.
type ChannelModel struct {
	ID, Name        string
	Context, Output int // tokens; 0 unknown
	Efforts         []string
}

// Quota is an account login's remaining share of its windows, in percent;
// negative for a window it does not report.
type Quota struct{ FiveHour, SevenDay float64 }

// Channel is a configured provider (data.rs Channel).
type Channel struct {
	ID, Name, Brand string
	Method          string // the brand's method ID
	API, Endpoint   string
	Auth            Auth
	Enabled         bool
	KeySet          bool   // a saved key; never loaded
	SignedIn        bool   // an account login holds a token
	Account         string // the signed-in account
	Models          []ChannelModel
	Default         int
	Quota           *Quota
}

// Method is a way a brand is reached (directory entry route): its label,
// how it is accessed, its API and endpoint.
type Method struct {
	ID, Name    string
	Access      string // provider_access_key, _endpoint, _login, _cloud
	API         string
	Endpoint    string
	Custom      bool   // the endpoint is the user's
	Login       string // for account logins: provider_login_claude, _chat_gpt
	KeyURL      string // where to get a key
	OAuthFields bool   // the OAuth advanced fields
}

// Brand is a provider family in the directory (presets.rs groups).
type Brand struct {
	ID, Name string
	Methods  []Method
}

// Catalog is the models.dev catalog's state (catalog.rs).
type Catalog struct {
	Updated, Scores bool
	Last            string // "2026-11-08 09:30"; "" never
}

// Providers is the providers page's dataset.
type Providers struct {
	Channels   []Channel
	Brands     []Brand
	Catalog    Catalog
	Compaction float64 // the threshold, percent
}

// ProvidersData builds the providers' dataset, fresh each call.
func ProvidersData() *Providers {
	efforts := []string{"low", "medium", "high"}
	return &Providers{
		Catalog: Catalog{Updated: true, Scores: true, Last: "2026-11-08 09:30"}, Compaction: 80,
		Channels: []Channel{
			{ID: "anthropic", Name: "Anthropic", Brand: "anthropic", Method: "claude-login", API: "Anthropic Messages", Auth: AuthOAuth,
				Enabled: true, SignedIn: true, Account: "dev@example.com", Quota: &Quota{FiveHour: 62, SevenDay: 81},
				Models: []ChannelModel{
					{ID: "claude-opus-5-5", Name: "Claude Opus 5.5", Context: 1000000, Output: 64000, Efforts: efforts},
					{ID: "claude-sonnet-5-5", Name: "Claude Sonnet 5.5", Context: 1000000, Output: 64000, Efforts: efforts},
					{ID: "claude-haiku-5-5", Name: "Claude Haiku 5.5", Context: 200000, Output: 32000},
				}},
			{ID: "openai", Name: "OpenAI", Brand: "openai", Method: "openai-key", API: "OpenAI Responses", Auth: AuthKey, Enabled: true, KeySet: true,
				Models: []ChannelModel{
					{ID: "gpt-5.2", Name: "GPT-5.2", Context: 400000, Output: 128000, Efforts: efforts},
					{ID: "gpt-5.2-mini", Name: "GPT-5.2 mini", Context: 400000, Output: 128000, Efforts: efforts},
					{ID: "o5", Name: "o5", Context: 200000, Output: 100000, Efforts: efforts},
					{ID: "gpt-4.1", Name: "GPT-4.1", Context: 1000000, Output: 32000},
					{ID: "gpt-4.1-mini", Name: "GPT-4.1 mini", Context: 1000000, Output: 32000},
					{ID: "gpt-image-2", Name: "GPT Image 2"},
					{ID: "text-embedding-4", Name: "Embedding 4"},
				}},
			{ID: "google", Name: "Google", Brand: "google", Method: "gemini-key", API: "Gemini", Auth: AuthKey,
				Models: []ChannelModel{{ID: "gemini-3-pro", Name: "Gemini 3 Pro", Context: 2000000, Output: 64000, Efforts: efforts}}},
			{ID: "openrouter", Name: "OpenRouter", Brand: "openrouter", Method: "openrouter-key", API: "OpenAI Chat", Auth: AuthKey, KeySet: true,
				Models: []ChannelModel{{ID: "deepseek/deepseek-v4", Name: "DeepSeek V4"}, {ID: "qwen/qwen3-max", Name: "Qwen3 Max"}}},
			{ID: "ollama", Name: "Ollama", Brand: "ollama", Method: "ollama-local", API: "OpenAI Chat", Endpoint: "http://localhost:11434/v1", Auth: AuthNone, Enabled: true,
				Models: []ChannelModel{{ID: "qwen3:32b", Name: "qwen3:32b", Context: 32768}}},
			{ID: "deepseek", Name: "DeepSeek", Brand: "deepseek", Method: "chatgpt-login", API: "OpenAI Responses", Auth: AuthOAuth},
		},
		Brands: []Brand{
			{ID: "anthropic", Name: "Anthropic", Methods: []Method{
				{ID: "claude-login", Name: "Claude", Access: "provider_access_login", API: "Anthropic Messages", Login: "provider_login_claude", OAuthFields: true},
				{ID: "anthropic-key", Name: "Anthropic API", Access: "provider_access_key", API: "Anthropic Messages", KeyURL: "https://console.anthropic.com/settings/keys"},
				{ID: "bedrock", Name: "Amazon Bedrock", Access: "provider_access_cloud", API: "Anthropic Messages"},
			}},
			{ID: "openai", Name: "OpenAI", Methods: []Method{
				{ID: "chatgpt-login", Name: "ChatGPT", Access: "provider_access_login", API: "OpenAI Responses", Login: "provider_login_chat_gpt"},
				{ID: "openai-key", Name: "OpenAI API", Access: "provider_access_key", API: "OpenAI Responses", KeyURL: "https://platform.openai.com/api-keys"},
				{ID: "azure", Name: "Azure OpenAI", Access: "provider_access_endpoint", API: "OpenAI Responses", Custom: true},
			}},
			{ID: "google", Name: "Google", Methods: []Method{
				{ID: "gemini-key", Name: "Gemini API", Access: "provider_access_key", API: "Gemini", KeyURL: "https://aistudio.google.com/apikey"},
			}},
			{ID: "deepseek", Name: "DeepSeek", Methods: []Method{
				{ID: "deepseek-key", Name: "DeepSeek API", Access: "provider_access_key", API: "OpenAI Chat"},
			}},
			{ID: "openrouter", Name: "OpenRouter", Methods: []Method{
				{ID: "openrouter-key", Name: "OpenRouter", Access: "provider_access_key", API: "OpenAI Chat"},
			}},
			{ID: "ollama", Name: "Ollama", Methods: []Method{
				{ID: "ollama-local", Name: "Ollama", Access: "provider_access_endpoint", API: "OpenAI Chat", Endpoint: "http://localhost:11434/v1", Custom: true},
			}},
			{ID: "custom", Name: "Custom", Methods: []Method{
				{ID: "openai-compatible", Name: "OpenAI compatible", Access: "provider_access_endpoint", API: "OpenAI Chat", Custom: true},
				{ID: "anthropic-compatible", Name: "Anthropic compatible", Access: "provider_access_endpoint", API: "Anthropic Messages", Custom: true},
			}},
		},
	}
}

// Discovered are the models a provider's model listing returns in the
// preview, for the editor's Discover.
var Discovered = []ChannelModel{
	{ID: "model-large", Name: "model-large", Context: 200000, Output: 32000},
	{ID: "model-small", Name: "model-small", Context: 128000, Output: 16000},
}

// ConnState is a connector account's state (connector::State).
type ConnState int

const (
	Unconnected ConnState = iota
	Connecting
	Connected
	Expired
	NeedsConfirmation
	ConnFailed
)

// ServerState is an MCP server's runtime state; ServerAuth is a server
// waiting for its connector's credentials (Availability::ConnectionRequired).
type ServerState int

const (
	ServerUp ServerState = iota
	ServerStarting
	ServerDown
	ServerAuth
)

// Server is an MCP server of a connector or added by hand.
type Server struct {
	Name      string
	Transport int // 0 stdio, 1 streamable HTTP, 2 SSE
	Command   string
	Args      []string
	URL       string
	Env       []EnvVar // environment (stdio) or headers (HTTP); values are never loaded
	Allowed   bool
	State     ServerState
	Tools     int
	Updated   string // "2026-11-09 11:42:10"
	Error     string
}

// ConnectorField is a value a connector asks for.
type ConnectorField struct {
	Name, Title string
	Secret      bool
	Help        bool // the field links to its help page
}

// ConnectorType is a connector a plugin declares (catalog types): its
// methods by i18n key (connector_oauth, _token, _application, _cli) and
// fields.
type ConnectorType struct {
	ID, Package, Name, Description string
	Methods                        []string
	Fields                         []ConnectorField
	Enabled                        bool
}

// Service is a row of the Connectors page (catalog.rs Service): a
// connector's account, its MCP servers, or an MCP server added by hand
// (Manual).
type Service struct {
	ID, Type, Name, Description string
	Identity                    string // the account's identity
	Method                      string // the account's method key; "" none
	Account                     bool
	State                       ConnState
	AccountOn                   bool
	Manual                      bool
	Servers                     []Server
}

// Connectors is the connectors page's dataset.
type Connectors struct {
	Types    []ConnectorType
	Services []Service
	Accounts []string // local CLI accounts a host reports
}

// ConnectorsData builds the connectors' dataset, fresh each call.
func ConnectorsData() *Connectors {
	return &Connectors{
		Types: []ConnectorType{
			{ID: "github", Package: "sailry-github", Name: "GitHub", Description: "Repositories, issues and pull requests",
				Methods: []string{"connector_oauth", "connector_token", "connector_cli"}, Enabled: true,
				Fields: []ConnectorField{{Name: "token", Title: "Personal access token", Secret: true, Help: true}}},
			{ID: "linear", Package: "sailry-linear", Name: "Linear", Description: "Issues and projects", Methods: []string{"connector_oauth"}, Enabled: true},
			{ID: "slack", Package: "sailry-slack", Name: "Slack", Description: "Channels and messages",
				Methods: []string{"connector_application"}, Enabled: true,
				Fields: []ConnectorField{{Name: "client_id", Title: "Client ID"}, {Name: "client_secret", Title: "Client secret", Secret: true}}},
			{ID: "notion", Package: "sailry-notion", Name: "Notion", Description: "Pages and databases", Methods: []string{"connector_token"}, Enabled: true,
				Fields: []ConnectorField{{Name: "token", Title: "Integration token", Secret: true}}},
			{ID: "sentry", Package: "sailry-sentry", Name: "Sentry", Description: "Errors and releases", Methods: []string{"connector_oauth"}},
		},
		Services: []Service{
			{ID: "github", Type: "github", Name: "GitHub", Description: "Repositories, issues and pull requests", Identity: "octo-dev",
				Method: "connector_oauth", Account: true, State: Connected, AccountOn: true,
				Servers: []Server{{Name: "github", Transport: 1, URL: "https://api.githubcopilot.com/mcp/", Allowed: true, Tools: 38, Updated: "2026-11-09 11:42:10"}}},
			{ID: "linear", Type: "linear", Name: "Linear", Description: "Issues and projects", Method: "connector_oauth", Account: true,
				State: Connecting, AccountOn: true},
			{ID: "slack", Type: "slack", Name: "Slack", Description: "Channels and messages", Identity: "Acme workspace",
				Method: "connector_application", Account: true, State: Expired, AccountOn: true},
			{ID: "notion", Type: "notion", Name: "Notion", Description: "Pages and databases",
				Servers: []Server{{Name: "notion", Transport: 1, URL: "https://mcp.notion.com/mcp", Allowed: true, State: ServerAuth}}},
			{ID: "mcp-filesystem", Name: "filesystem", Description: "npx @modelcontextprotocol/server-filesystem", Manual: true,
				Servers: []Server{{Name: "mcp-filesystem", Command: "npx", Args: []string{"-y", "@modelcontextprotocol/server-filesystem", "~/Projects"},
					Allowed: true, State: ServerDown, Tools: 11, Updated: "2026-11-09 10:05:33", Error: "spawn npx ENOENT"}}},
			{ID: "mcp-search", Name: "search", Description: "https://search.example.com/mcp", Manual: true,
				Servers: []Server{{Name: "mcp-search", Transport: 1, URL: "https://search.example.com/mcp", Allowed: true, State: ServerStarting,
					Env: []EnvVar{{Key: "Authorization"}}}}},
		},
		Accounts: []string{"octo-dev (github.com)", "octo-work (github.example.com)"},
	}
}
