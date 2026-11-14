package preview

import "github.com/sailry/sailry-harness/internal/desktop/viewmodel"

// The AI settings' preview data: providers, connectors, teams, plugins,
// skills, the market, MCP servers and the model library, as a Node would
// report them. Credentials are never part of it.

// Auth is how a provider channel signs in.
type Auth = viewmodel.Auth

const (
	AuthKey    = viewmodel.AuthKey
	AuthOAuth  = viewmodel.AuthOAuth
	AuthPlugin = viewmodel.AuthPlugin
	AuthNone   = viewmodel.AuthNone
)

// ChannelModel is a model a channel offers.
type ChannelModel = viewmodel.ChannelModel

// ModelFeature is a feature a model declares.
type ModelFeature = viewmodel.ModelFeature

// Quota is an account login's remaining share of its windows, in percent;
// negative for a window it does not report.
type Quota = viewmodel.Quota

// Channel is a configured provider.
type Channel = viewmodel.Channel

// Method is a way a brand is reached (directory entry route): its label,
// how it is accessed, its API and endpoint.
type Method = viewmodel.Method
type Route = viewmodel.Route

// Brand is a provider family in the directory.
type Brand = viewmodel.Brand

// Catalog is the models.dev catalog's state.
type Catalog = viewmodel.Catalog

// Providers is the providers page's dataset.
type Providers struct {
	Channels   []Channel
	Brands     []Brand
	Catalog    Catalog
	Compaction float64 // the threshold, percent
	// Media are the enabled channels' models the media settings offer.
	Media []viewmodel.MediaOption
}

// ProvidersData builds the providers' dataset, fresh each call.
func ProvidersData() *Providers {
	efforts := []string{"low", "medium", "high"}
	d := &Providers{
		Catalog: Catalog{Updated: true, Scores: true, Last: "2026-11-08 09:30"}, Compaction: 80,
		Channels: []Channel{
			{ID: "anthropic", Name: "Anthropic", Brand: "anthropic", Method: "claude-login", API: "Anthropic Messages", Auth: AuthOAuth,
				Enabled: true, SignedIn: true, Account: "dev@example.com", Quota: &Quota{FiveHour: 62, SevenDay: 81},
				Models: []ChannelModel{
					{ID: "claude-opus-5-5", Name: "Claude Opus 5.5", Context: 1000000, Output: 64000, Efforts: efforts, Vision: true, Tools: true, Reasoning: true},
					{ID: "claude-sonnet-5-5", Name: "Claude Sonnet 5.5", Context: 1000000, Output: 64000, Efforts: efforts, Vision: true, Tools: true, Reasoning: true},
					{ID: "claude-haiku-5-5", Name: "Claude Haiku 5.5", Context: 200000, Output: 32000, Vision: true, Tools: true},
				}},
			{ID: "openai", Name: "OpenAI", Brand: "openai", Method: "openai-key", API: "OpenAI Responses", Auth: AuthKey, Enabled: true, KeySet: true,
				Models: []ChannelModel{
					{ID: "gpt-5.2", Name: "GPT-5.2", Context: 400000, Output: 128000, Efforts: efforts, Vision: true, Tools: true, Reasoning: true, Generates: []string{viewmodel.GenerateImageTool}},
					{ID: "gpt-5.2-mini", Name: "GPT-5.2 mini", Context: 400000, Output: 128000, Efforts: efforts, Vision: true, Tools: true, Reasoning: true},
					{ID: "o5", Name: "o5", Context: 200000, Output: 100000, Efforts: efforts, Tools: true, Reasoning: true},
					{ID: "gpt-4.1", Name: "GPT-4.1", Context: 1000000, Output: 32000, Vision: true, Tools: true},
					{ID: "gpt-4.1-mini", Name: "GPT-4.1 mini", Context: 1000000, Output: 32000, Vision: true, Tools: true},
					{ID: "gpt-image-2", Name: "GPT Image 2", Generates: []string{viewmodel.GenerateImage}},
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
				{ID: "claude-login", Name: "Claude", Access: "provider_access_login", API: "Anthropic Messages", Login: "provider_login_claude", OAuth: [4]string{1: "claude-cli/2.1.280 (external, cli)"}, Icon: "claude"},
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
				{ID: "openai-compatible", Name: "OpenAI compatible", Access: "provider_access_endpoint", Routes: []Route{
					{ID: "openai-compatible", API: "responses", Custom: true},
					{ID: "openai-compatible-chat", API: "chat_completions", Custom: true},
				}},
				{ID: "anthropic-compatible", Name: "Anthropic compatible", Access: "provider_access_endpoint", API: "Anthropic Messages", Custom: true},
			}},
		},
	}
	for _, ch := range d.Channels {
		for _, x := range ch.Models {
			if kinds := viewmodel.Capabilities(x.Vision, x.Generates); ch.Enabled && len(kinds) > 0 {
				d.Media = append(d.Media, viewmodel.MediaOption{Key: ch.ID + "/" + x.ID, Label: ch.Name + " · " + x.Name, Kinds: kinds})
			}
		}
	}
	return d
}

// Discovered are the models a provider's model listing returns in the
// preview, for the editor's Discover.
var Discovered = []ChannelModel{
	{ID: "model-large", Name: "model-large", Context: 200000, Output: 32000},
	{ID: "model-small", Name: "model-small", Context: 128000, Output: 16000},
}

// ConnState is a connector account's state (connector::State).
type ConnState = viewmodel.ConnState

const (
	Unconnected       = viewmodel.Unconnected
	Connecting        = viewmodel.Connecting
	Connected         = viewmodel.Connected
	Expired           = viewmodel.Expired
	NeedsConfirmation = viewmodel.NeedsConfirmation
	ConnFailed        = viewmodel.ConnFailed
)

// ServerState is an MCP server's runtime state; ServerAuth is a server
// waiting for its connector's credentials (Availability::ConnectionRequired).
type ServerState = viewmodel.ServerState

const (
	ServerUp       = viewmodel.ServerUp
	ServerStarting = viewmodel.ServerStarting
	ServerDown     = viewmodel.ServerDown
	ServerAuth     = viewmodel.ServerAuth
	ServerIdle     = viewmodel.ServerIdle
)

// Server is an MCP server of a connector or added by hand.
type Server = viewmodel.ConnectorServer

// ConnectorField is a value a connector asks for.
type ConnectorField = viewmodel.ConnectorField

// ConnectorType is a connector a plugin declares (catalog types): its
// methods by i18n key (connector_oauth, _token, _application, _cli) and
// fields.
type ConnectorType = viewmodel.ConnectorType

// Service is a row of the Connectors page: a connector's account, its MCP
// servers, or an MCP server added by hand (Manual).
type Service = viewmodel.ConnectorService

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

// Role is a subagent role (role::Profile).
type Role = viewmodel.Role

// Team is a team of roles led by a model (team::Profile).
type Team = viewmodel.Team

// TeamSet is the teams page's dataset.
type TeamSet struct {
	Teams       []Team
	Roles       []Role
	Concurrency float64
}

// TeamsData builds the teams' dataset, fresh each call.
func TeamsData() *TeamSet {
	return &TeamSet{
		Concurrency: 4,
		Roles: []Role{
			{ID: "explore", Key: "explore", Name: "Explore code", Description: "Read the current project's code and report findings", ReadOnly: true, Avatar: "avatar-fox"},
			{ID: "research", Key: "research", Name: "Web research", Description: "Investigate public web sources", ReadOnly: true, Avatar: "avatar-owl"},
			{ID: "reviewer", Key: "code-reviewer", Name: "Reviewer", Description: "Review changes and report evidence-backed findings",
				Model: "anthropic/claude-opus-5-5", Effort: "high", ReadOnly: true, Instructions: "Review code and changes within the assigned scope.", Avatar: "avatar-panda"},
			{ID: "tester", Key: "tester", Name: "Tester", Description: "Run checks and report verified results", Instructions: "Verify the assigned change."},
		},
		Teams: []Team{
			{ID: "ship", Name: "Ship it", Description: "Implement, test and review", Members: []string{"explore", "tester", "reviewer"},
				Lead: "anthropic/claude-sonnet-5-5", Effort: "medium"},
			{ID: "study", Name: "Study", Description: "Research before a change", Members: []string{"explore", "research"}},
		},
	}
}

// PackageKind classifies a package by what it provides: plugins show in
// Plugins, skill packages in Skills, MCP packages in Connectors.
type PackageKind = viewmodel.PackageKind

const (
	KindPlugin = viewmodel.KindPlugin
	KindSkill  = viewmodel.KindSkill
	KindMCP    = viewmodel.KindMCP
)

// Setting is a plugin configuration field: Kind is "text", "secret", "bool",
// "choice" or "number". A secret's value is never loaded; Set says one is
// saved.
type Setting = viewmodel.Setting

// Package is an installed or catalog plugin (plugin::Summary and Info).
type Package = viewmodel.Package

// Skill is an installed skill.
type Skill = viewmodel.Skill

// MarketTheme is a theme in the market.
type MarketTheme = viewmodel.MarketTheme

type LibraryModel = viewmodel.LibraryModel

// Extensions is the plugins, skills, market and model library dataset.
type Extensions struct {
	Packages []Package // installed
	Catalog  []Package // the market's plugins
	Skills   []Skill
	Market   []Skill // the market's skills
	Themes   []MarketTheme
	Library  []LibraryModel
}

// ExtensionsData builds the extensions' dataset, fresh each call.
func ExtensionsData() *Extensions {
	notes := "- Branch picker in the composer\n- Faster diffs for large files\n- Fixes a crash on empty repositories"
	git := Package{ID: "git", Name: "Git", Description: "Branches, diffs and commits in conversations", Version: "0.3.2", Latest: "0.3.3",
		Icon: "phosphor:git-branch", Tint: "amber", Category: "market_category_development", Source: "market_details_source_official", Official: true, Enabled: true, Installed: true,
		Tools: []string{"git_status", "git_diff", "git_commit"}, Skills: []string{"commit-message"},
		Permissions: []string{"market_permission_git_read", "market_permission_git_write", "market_permission_files_read"}, Notes: notes,
		Settings: []Setting{
			{Name: "author", Title: "Commit author", Kind: "text", Value: "Dev <dev@example.com>"},
			{Name: "sign", Title: "Sign commits", Kind: "bool", Value: "true"},
			{Name: "model", Title: "Message model", Kind: "choice", Options: []string{"Follow model", "Claude Haiku 5.5", "GPT-5.2 mini"}, Value: "Follow model"},
			{Name: "token", Title: "GitHub token", Kind: "secret", Set: true},
		},
		Versions: []string{"0.3.3", "0.3.2", "0.3.1", "0.2.0"}}
	return &Extensions{
		Packages: []Package{
			git,
			{ID: "terminal", Name: "Terminal", Description: "Ghostty terminals beside the conversation", Version: "0.2.0", Latest: "0.2.0",
				Icon: "phosphor:terminal-window", Tint: "teal", Category: "market_category_development", Source: "market_details_source_official", Official: true, Enabled: true, Installed: true,
				Tools: []string{"terminal_run", "terminal_read"}, Permissions: []string{"market_permission_terminals_control"}, Versions: []string{"0.2.0"}},
			{ID: "browser", Name: "Browser", Description: "Pages the agent reads and annotates", Version: "0.1.4", Latest: "0.2.0", Requires: "0.2.0",
				Icon: "phosphor:browser", Tint: "blue", Category: "market_category_automation", Source: "market_details_source_official", Official: true, Installed: true,
				Tools: []string{"browser_open", "browser_read"}, Permissions: []string{"market_permission_browser_control", "market_permission_http_request"},
				Notes: "- Needs the new web view host", Versions: []string{"0.2.0", "0.1.4"}},
			{ID: "figma", Name: "Figma", Description: "Frames and comments from design files", Version: "0.1.0", Latest: "0.1.0",
				Icon: "ailogo:figma", Tint: "violet", Category: "market_category_documents", Source: "market_details_source_community", Installed: true,
				MCP: []string{"figma"}, Permissions: []string{"market_permission_http_request"}, Versions: []string{"0.1.0"},
				Settings: []Setting{{Name: "team", Title: "Team ID", Kind: "text", Required: true}, {Name: "limit", Title: "Frame limit", Kind: "number", Value: "50"}}},
			{ID: "linear", Name: "Linear", Description: "Issues linked to conversations", Version: "0.2.3", Latest: "0.2.3",
				Category: "market_category_automation", Source: "market_details_source_local", Enabled: true, Installed: true,
				Tools: []string{"linear_issue"}, Versions: []string{"0.2.3"}},
			{ID: "review-skills", Name: "Review skills", Description: "Skills for code review", Version: "1.0.0", Latest: "1.0.0", Kind: KindSkill,
				Source: "market_details_source_official", Official: true, Enabled: true, Installed: true, Skills: []string{"review", "security-review"}},
			{ID: "mcp-github", Name: "GitHub MCP", Description: "GitHub's MCP server", Version: "0.4.0", Latest: "0.4.0", Kind: KindMCP,
				Source: "market_details_source_official", Official: true, Enabled: true, Installed: true, MCP: []string{"github"}},
		},
		Catalog: []Package{
			git,
			{ID: "jira", Name: "Jira", Description: "Issues and sprints from Jira Cloud", Version: "0.2.1", Latest: "0.2.1", Icon: "ailogo:jira", Tint: "blue", Category: "market_category_automation",
				Source: "market_details_source_official", Official: true, Tools: []string{"jira_issue"}, Permissions: []string{"market_permission_http_request"}, Versions: []string{"0.2.1"}},
			{ID: "postgres", Name: "Postgres", Description: "Query databases from the conversation", Version: "0.3.0", Latest: "0.3.0", Icon: "phosphor:database", Tint: "teal", Category: "market_category_data",
				Source: "market_details_source_official", Official: true, Tools: []string{"sql_query"}, Permissions: []string{"market_permission_databases_control"}, Versions: []string{"0.3.0"}},
			{ID: "chess", Name: "Chess", Description: "Play a game while the agent works", Version: "0.1.2", Latest: "0.1.2", Icon: "phosphor:crown", Tint: "amber", Category: "market_category_entertainment",
				Source: "market_details_source_community", Versions: []string{"0.1.2"}},
			{ID: "pdf", Name: "PDF tools", Description: "Split, merge and read PDF files", Version: "0.1.0", Latest: "0.1.0", Icon: "phosphor:file-pdf", Tint: "violet", Category: "market_category_documents",
				Source: "market_details_source_community", Tools: []string{"pdf_read"}, Permissions: []string{"market_permission_files_read"}, Versions: []string{"0.1.0"}},
		},
		Skills: []Skill{
			{ID: "review", Name: "review", Description: "Review a diff for bugs", Package: "Review skills", Enabled: true},
			{ID: "security-review", Name: "security-review", Description: "Review changes for security issues", Package: "Review skills", Enabled: true},
			{ID: "release-notes", Name: "release-notes", Description: "Write release notes from commits", Origin: "github.com/sailry/skills", Enabled: true, Update: true},
			{ID: "sql-style", Name: "sql-style", Description: "House style for SQL", Origin: "skills.sh/acme/sql-style"},
		},
		Market: []Skill{
			{ID: "release-notes", Name: "release-notes", Description: "Write release notes from commits", Origin: "github.com/sailry/skills", Update: true},
			{ID: "pr-describe", Name: "pr-describe", Description: "Describe a pull request", Origin: "github.com/sailry/skills"},
			{ID: "changelog", Name: "changelog", Description: "Keep a changelog", Origin: "skills.sh/keep/changelog"},
		},
		Themes: []MarketTheme{
			{ID: "neutral", Name: "Neutral", Description: "Sailry's default", Dark: true, Light: true, Installed: true, Builtin: true, Current: true},
			{ID: "paper", Name: "Paper", Description: "Warm light theme", Light: true, Installed: true, Update: true},
			{ID: "midnight", Name: "Midnight", Description: "Deep blue dark theme", Dark: true},
			{ID: "forest", Name: "Forest", Description: "Greens for day and night", Dark: true, Light: true},
		},
		Library: []LibraryModel{
			{ID: "bge-m3", Name: "BGE-M3", Repo: "BAAI/bge-m3", Kind: "model_library_embedding", Source: "model_library_huggingface", Size: "2.3 GB", License: "MIT", Status: ModelInstalled},
			{ID: "qwen3-embed", Name: "Qwen3 Embedding 0.6B", Repo: "Qwen/Qwen3-Embedding-0.6B", Kind: "model_library_embedding", Source: "model_library_modelscope", Size: "1.2 GB", License: "Apache-2.0", Status: ModelDownloading, Percent: 27},
			{ID: "ppocr", Name: "PP-OCRv5", Repo: "PaddlePaddle/PP-OCRv5_mobile", Kind: "model_library_ocr", Source: "model_library_huggingface", Size: "16 MB", License: "Apache-2.0", Status: ModelInstalled},
			{ID: "whisper-small", Name: "Whisper Small", Repo: "openai/whisper-small", Kind: "model_library_speech", Source: "model_library_huggingface", Size: "967 MB", License: "Apache-2.0"},
			{ID: "qwen3-0.6b", Name: "Qwen3 0.6B", Repo: "Qwen/Qwen3-0.6B", Kind: "model_library_decision", Source: "model_library_huggingface", Size: "1.5 GB", License: "Apache-2.0"},
			{ID: "bge-small-zh", Name: "BGE Small Chinese", Repo: "BAAI/bge-small-zh-v1.5", Kind: "model_library_embedding", Source: "model_library_huggingface", Size: "96 MB", License: "MIT"},
			{ID: "granite-embed", Name: "Granite Multilingual", Repo: "ibm-granite/granite-embedding-97m-multilingual-r2", Kind: "model_library_embedding", Source: "model_library_huggingface", Size: "390 MB", License: "Apache-2.0"},
			{ID: "rapidocr", Name: "RapidOCR", Repo: "RapidAI/RapidOCR", Kind: "model_library_ocr", Source: "model_library_modelscope", Size: "15 MB", License: "Apache-2.0"},
			{ID: "whisper-turbo", Name: "Whisper Large v3 Turbo", Repo: "openai/whisper-large-v3-turbo", Kind: "model_library_speech", Source: "model_library_huggingface", Size: "1.6 GB", License: "MIT"},
			{ID: "qwen3-1.7b", Name: "Qwen3 1.7B", Repo: "Qwen/Qwen3-1.7B", Kind: "model_library_decision", Source: "model_library_huggingface", Size: "3.4 GB", License: "Apache-2.0"},
		},
	}
}
