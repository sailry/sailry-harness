package hostctl

import (
	"errors"
	"io"
	"net"
	"net/netip"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const ServiceUsage = "Usage: sailry-host [--data-dir <absolute private directory>] [--bind <IP:port> | --internet | --relay <HTTPS URL> ...] [--pairing-service <HTTPS origin> | --bootstrap] [--telemetry on|off]\n       sailry-host --version\n       sailry-host share [--data-dir <profile>] [--pairing-service <HTTPS origin>]\n       sailry-host submit --help\n       sailry-host wait --help\nDefaults to ~/.sailry-go and loopback. No interactive Agent.\nPairing is opt-in: prints a private PIN, refreshes every 60 seconds until paired or stopped."

const TaskUsage = "Usage: sailry-host submit (--session <id> | --worktree <id>) (--prompt <text> | --prompt-file <path> | --stdin) [--request-id <id>] [--expected-revision <number>] [--config <JSON file>] [--execution model|team] [--team <id>] [--team-revision <number>] [--association <JSON file>] [--title <text>] [--no-wait] [--timeout <seconds>] [--data-dir <profile>]\n       sailry-host wait --session <id> --turn <id> [--timeout <seconds>] [--data-dir <profile>]\nWrites one JSON result. Exit codes: 0 admitted/completed, 1 failed/stopped, 3 approval/input/paused, 4 uncertain, 124 timeout, 130 observation cancelled. Waiting never grants approvals or stops execution."

type ServiceOptions struct {
	Profile        string
	Network        link.NetworkScope
	Bootstrap      bool
	PairingService string
	Telemetry      *bool
}

func ParseService(args []string) (ServiceOptions, bool, error) {
	options := ServiceOptions{Network: link.NetworkScope{Bind: "127.0.0.1:0"}}
	if len(args) != 0 && (args[0] == "--help" || args[0] == "-h") {
		if len(args) != 1 {
			return options, false, errors.New("--help cannot be combined with other options")
		}
		return options, true, nil
	}
	seen := map[string]bool{}
	for index := 0; index < len(args); index++ {
		flag := args[index]
		if seen[flag] && flag != "--relay" {
			return options, false, errors.New("unknown or repeated option; use --help for usage")
		}
		seen[flag] = true
		switch flag {
		case "--internet":
			options.Network.Internet = true
		case "--bootstrap":
			options.Bootstrap = true
		case "--data-dir", "--bind", "--relay", "--pairing-service", "--telemetry":
			if index+1 == len(args) {
				return options, false, errors.New(flag + " requires a value")
			}
			index++
			value := args[index]
			switch flag {
			case "--data-dir":
				if !filepath.IsAbs(value) {
					return options, false, errors.New("--data-dir must be an absolute path")
				}
				options.Profile = value
			case "--bind":
				if _, err := netip.ParseAddrPort(value); err != nil {
					return options, false, errors.New("--bind requires a valid IP:port address")
				}
				options.Network.Bind = value
			case "--relay":
				if err := validateRelay(value); err != nil {
					return options, false, err
				}
				options.Network.Relays = append(options.Network.Relays, value)
			case "--pairing-service":
				if err := validateOrigin(value); err != nil {
					return options, false, err
				}
				options.PairingService = value
			case "--telemetry":
				if value != "on" && value != "off" {
					return options, false, errors.New("--telemetry requires on or off")
				}
				enabled := value == "on"
				options.Telemetry = &enabled
			}
		default:
			return options, false, errors.New("unknown or repeated option; use --help for usage")
		}
	}
	if seen["--bind"] && (options.Network.Internet || len(options.Network.Relays) > 0) || options.Network.Internet && len(options.Network.Relays) > 0 {
		return options, false, errors.New("choose only one of --bind, --internet or --relay")
	}
	if options.Bootstrap && options.PairingService != "" {
		return options, false, errors.New("choose bootstrap or pairing service")
	}
	if options.Network.Internet || len(options.Network.Relays) > 0 {
		options.Network.Bind = ""
	}
	if options.Profile == "" {
		var err error
		options.Profile, err = core.DefaultDataDir()
		if err != nil {
			return options, false, err
		}
	}
	return options, false, nil
}

func validateOrigin(origin string) error {
	parsed, err := url.Parse(origin)
	if err != nil || parsed.Hostname() == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || parsed.Path != "" && parsed.Path != "/" {
		return errors.New("invalid pairing service origin")
	}
	if parsed.Scheme == "https" {
		return nil
	}
	if parsed.Scheme == "http" {
		ip := net.ParseIP(parsed.Hostname())
		if parsed.Hostname() == "localhost" || ip != nil && ip.IsLoopback() {
			return nil
		}
	}
	return errors.New("pairing service requires HTTPS or loopback HTTP")
}

func validateRelay(value string) error {
	parsed, err := url.Parse(value)
	if err != nil || parsed.Scheme != "https" || parsed.Hostname() == "" || parsed.User != nil || parsed.Fragment != "" {
		return errors.New("relay requires an HTTPS URL")
	}
	return nil
}

type TaskOptions struct {
	Profile string
	Task    Task
	Wait    bool
}

func ParseTask(args []string, stdin io.Reader) (TaskOptions, bool, error) {
	options := TaskOptions{Wait: true}
	if len(args) == 0 || args[0] != "submit" && args[0] != "wait" {
		return options, false, errors.New(TaskUsage)
	}
	command := args[0]
	values := map[string]string{}
	flags := map[string]bool{}
	for index := 1; index < len(args); index++ {
		flag := args[index]
		if flag == "--help" || flag == "-h" {
			return options, true, nil
		}
		if flags[flag] {
			return options, false, errors.New("unknown or repeated task option")
		}
		flags[flag] = true
		if command == "submit" && (flag == "--stdin" || flag == "--no-wait") {
			continue
		}
		allowed := flag == "--data-dir" || flag == "--session" || flag == "--timeout"
		if command == "submit" {
			allowed = allowed || flag == "--worktree" || flag == "--request-id" || flag == "--expected-revision" || flag == "--prompt" || flag == "--prompt-file" || flag == "--config" || flag == "--execution" || flag == "--team" || flag == "--team-revision" || flag == "--association" || flag == "--title"
		} else {
			allowed = allowed || flag == "--turn"
		}
		if !allowed || index+1 == len(args) {
			return options, false, errors.New(TaskUsage)
		}
		index++
		values[flag] = args[index]
	}
	options.Profile = values["--data-dir"]
	if flags["--data-dir"] && !filepath.IsAbs(options.Profile) {
		return options, false, errors.New("--data-dir must be absolute")
	}
	if options.Profile == "" {
		var err error
		options.Profile, err = core.DefaultDataDir()
		if err != nil {
			return options, false, err
		}
	}
	timeout := uint64(600)
	if flags["--timeout"] {
		var err error
		timeout, err = strconv.ParseUint(values["--timeout"], 10, 64)
		if err != nil {
			return options, false, errors.New("--timeout must be between 1 and 86400 seconds")
		}
	}
	if timeout < 1 || timeout > 86400 {
		return options, false, errors.New("--timeout must be between 1 and 86400 seconds")
	}
	task := Task{Command: command, Version: 1, Timeout: timeout, Wait: !flags["--no-wait"]}
	options.Wait = task.Wait
	if command == "wait" {
		session, err := protocol.ParseSessionID(values["--session"])
		if err != nil {
			return options, false, errors.New("wait requires a valid --session")
		}
		turn, err := protocol.ParseTurnID(values["--turn"])
		if err != nil {
			return options, false, errors.New("wait requires a valid --turn")
		}
		task.Session, task.Turn = &session, &turn
		options.Task = task
		return options, false, nil
	}
	if flags["--session"] == flags["--worktree"] {
		return options, false, errors.New("choose one session or worktree")
	}
	if flags["--session"] && (flags["--config"] || flags["--title"] || flags["--execution"] || flags["--team"] || flags["--team-revision"] || flags["--association"]) || flags["--worktree"] && flags["--expected-revision"] {
		return options, false, errors.New("configuration/title/execution/association require a new worktree task; expected revision requires a session")
	}
	if flags["--team-revision"] && !flags["--team"] {
		return options, false, errors.New("--team-revision requires --team")
	}
	if values["--execution"] == "model" && flags["--team"] {
		return options, false, errors.New("model execution cannot select a team")
	}
	inputCount := 0
	var text string
	for _, flag := range []string{"--stdin", "--prompt", "--prompt-file"} {
		if !flags[flag] {
			continue
		}
		inputCount++
		var err error
		switch flag {
		case "--stdin":
			text, err = readInput(stdin)
		case "--prompt":
			text = values[flag]
		case "--prompt-file":
			text, err = readInputFile(values[flag])
		}
		if err != nil {
			return options, false, err
		}
	}
	if inputCount != 1 || strings.TrimSpace(text) == "" || len(text) > MaxTaskRequest || !utf8.ValidString(text) {
		return options, false, errors.New("choose one nonblank prompt source within the request limit")
	}
	task.Message = protocol.ConversationInput{Text: text, Attachments: []protocol.AttachmentID{}}
	task.Request = protocol.NewRequestID()
	if flags["--request-id"] {
		var err error
		task.Request, err = protocol.ParseRequestID(values["--request-id"])
		if err != nil {
			return options, false, err
		}
	}
	target := Target{}
	if flags["--session"] {
		session, err := protocol.ParseSessionID(values["--session"])
		if err != nil {
			return options, false, err
		}
		target.Kind, target.Session = "session", session
		if flags["--expected-revision"] {
			revision, err := strconv.ParseUint(values["--expected-revision"], 10, 64)
			if err != nil {
				return options, false, err
			}
			target.ExpectedRevision = &revision
		}
	} else {
		worktree, err := protocol.ParseWorktreeID(values["--worktree"])
		if err != nil {
			return options, false, err
		}
		target.Kind, target.Worktree, target.Execution = "worktree", worktree, protocol.ConversationExecutionTeam
		if flags["--execution"] {
			if values["--execution"] != "model" && values["--execution"] != "team" {
				return options, false, errors.New("--execution must be model or team")
			}
			target.Execution = protocol.ConversationExecution(values["--execution"])
		}
		if flags["--team"] {
			team, err := protocol.ParseTeamID(values["--team"])
			if err != nil {
				return options, false, err
			}
			target.Team = &protocol.TeamSelection{ID: team}
			if flags["--team-revision"] {
				revision, err := strconv.ParseUint(values["--team-revision"], 10, 64)
				if err != nil || strconv.FormatUint(revision, 10) != values["--team-revision"] {
					return options, false, errors.New("--team-revision must be a canonical decimal integer")
				}
				target.Team.Revision = &revision
			}
		}
		if flags["--config"] {
			target.Config = new(protocol.SessionConfig)
			if err := readJSONFile(values["--config"], target.Config); err != nil {
				return options, false, err
			}
		}
		if flags["--association"] {
			target.Association = new(protocol.ConversationAssociation)
			if err := readJSONFile(values["--association"], target.Association); err != nil {
				return options, false, err
			}
			if err := validateAssociation(*target.Association); err != nil {
				return options, false, err
			}
		}
		title := values["--title"]
		if !flags["--title"] {
			title = strings.SplitN(text, "\n", 2)[0]
			runes := []rune(title)
			title = string(runes[:min(len(runes), 80)])
		}
		task.Title = title
	}
	task.Target = target
	options.Task = task
	return options, false, nil
}

func readInput(reader io.Reader) (string, error) {
	body, err := io.ReadAll(io.LimitReader(reader, MaxTaskRequest+1))
	if err != nil {
		return "", err
	}
	if len(body) > MaxTaskRequest {
		return "", errors.New("task input exceeds its limit")
	}
	if !utf8.Valid(body) {
		return "", errors.New("task input must be UTF-8")
	}
	return string(body), nil
}

func readInputFile(path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	return readInput(file)
}
