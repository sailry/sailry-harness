package hostctl

import (
	"bufio"
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/mattn/go-isatty"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
)

type CLIHandlers struct {
	Identity   identity.Desktop
	Service    func(context.Context, []string) (int, error)
	Acceptance func(context.Context, []string) (int, error)
}

// RunCLI dispatches Host commands. Legacy task commands keep their established
// parsers; new interactive flows use this small command table.
func RunCLI(ctx context.Context, args []string, stdin io.Reader, stdout, stderr io.Writer, handlers CLIHandlers) (int, error) {
	input := bufio.NewReader(stdin)
	return runCLI(ctx, args, input, stdin, terminal(stdin), stdout, stderr, handlers)
}

func runCLI(ctx context.Context, args []string, input *bufio.Reader, stdin io.Reader, interactive bool, stdout, stderr io.Writer, handlers CLIHandlers) (int, error) {
	if len(args) == 0 {
		if interactive {
			command, err := runMenu(ctx, input, stdout, handlers.Identity)
			if err != nil || command == "" {
				return 0, err
			}
			return runCLI(ctx, []string{command}, input, stdin, interactive, stdout, stderr, handlers)
		}
		return 0, PrintHelp(stdout)
	}
	command, rest := args[0], args[1:]
	switch command {
	case "help", "--help", "-h":
		if len(rest) > 0 {
			return 1, errors.New("help does not take arguments")
		}
		return 0, PrintHelp(stdout)
	case "--version", "version":
		if len(rest) > 0 {
			return 1, errors.New("version does not take arguments")
		}
		_, err := fmt.Fprintln(stdout, versionLabel(handlers.Identity))
		return 0, err
	case "menu":
		flags := flag.NewFlagSet("menu", flag.ContinueOnError)
		flags.SetOutput(stderr)
		profile := flags.String("data-dir", "", "Host profile directory")
		if err := flags.Parse(rest); err != nil {
			return 1, err
		}
		if flags.NArg() != 0 || *profile != "" && !filepath.IsAbs(*profile) {
			return 1, errors.New("Usage: sailry menu [--data-dir <absolute profile>]")
		}
		if !interactive {
			return 0, PrintHelp(stdout)
		}
		selected, err := runMenu(ctx, input, stdout, handlers.Identity)
		if err != nil || selected == "" {
			return 0, err
		}
		selectedArgs := []string{selected}
		if selected == "link" && *profile != "" {
			selectedArgs = append(selectedArgs, "--data-dir", *profile)
		}
		return runCLI(ctx, selectedArgs, input, stdin, interactive, stdout, stderr, handlers)
	case "serve":
		return runService(ctx, rest, handlers)
	case "link":
		return 0, runLink(ctx, rest, input, interactive, stdout, stderr, handlers.Identity.DefaultProfile)
	case "share":
		return 0, runShare(ctx, rest, stdout, stderr, handlers.Identity.DefaultProfile)
	case "users", "devices":
		return runIdentity(ctx, command, rest, stdout, handlers.Identity.DefaultProfile)
	case "submit", "wait":
		return runTask(ctx, command, rest, input, stdout, handlers.Identity.DefaultProfile)
	case "start", "stop", "restart", "status", "update":
		if command != "update" && len(rest) > 0 {
			return 1, fmt.Errorf("%s does not take arguments", command)
		}
		return 0, runInstalledCommand(ctx, command, rest, handlers.Identity, stdin, stdout, stderr)
	case "acceptance", "acceptance-cleanup":
		if handlers.Acceptance == nil {
			return 1, errors.New("acceptance is unavailable")
		}
		if command == "acceptance-cleanup" {
			rest = append([]string{"--cleanup"}, rest...)
		}
		return handlers.Acceptance(ctx, rest)
	default:
		if strings.HasPrefix(command, "-") {
			return runService(ctx, args, handlers)
		}
		return 1, fmt.Errorf("unknown command %q; use sailry help", command)
	}
}

func runService(ctx context.Context, args []string, handlers CLIHandlers) (int, error) {
	if handlers.Service == nil {
		return 1, errors.New("Host service is unavailable")
	}
	return handlers.Service(ctx, args)
}

func versionLabel(product identity.Desktop) string {
	version := product.Version
	if version == "" {
		version = link.ProductVersion
	}
	return identity.OfficialHostVersionTag + version
}

func runIdentity(ctx context.Context, command string, args []string, stdout io.Writer, profile string) (int, error) {
	if len(args) == 1 && (args[0] == "--help" || args[0] == "-h") {
		_, err := fmt.Fprintln(stdout, IdentityUsage)
		return 0, err
	}
	return runIdentityCommand(ctx, append([]string{command}, args...), stdout, profile)
}

func runTask(ctx context.Context, command string, args []string, stdin io.Reader, stdout io.Writer, profile string) (int, error) {
	options, help, err := parseTask(append([]string{command}, args...), stdin, profile)
	if err != nil {
		return 1, err
	}
	if help {
		_, err := fmt.Fprintln(stdout, TaskUsage)
		return 0, err
	}
	return RunTask(ctx, options, stdout)
}

const CLIUsage = `Usage: sailry <command>

Commands:
  link       Select a relay and connect a device
  start      Start the installed Host service
  stop       Stop the installed Host service
  restart    Restart the installed Host service
  status     Show Host service status
  update     Update the installed Host
  version    Show the installed version
  share      Show a code on the current network (legacy)
  submit     Submit work to this Host
  wait       Observe submitted work
  users      Inspect Host users
  devices    Inspect paired devices
  help       Show this command menu

Run sailry without arguments in a terminal for an interactive menu.
Run sailry serve [Host options] to start a foreground Host.`

func PrintHelp(stdout io.Writer) error {
	_, err := fmt.Fprintln(stdout, CLIUsage)
	return err
}

func RunMenu(ctx context.Context, input *bufio.Reader, output io.Writer) (string, error) {
	return runMenu(ctx, input, output, identity.Desktop{})
}

func runMenu(ctx context.Context, input *bufio.Reader, output io.Writer, product identity.Desktop) (string, error) {
	commands := []struct{ command, label string }{
		{"link", "Connect a device"}, {"status", "Host status"}, {"start", "Start Host"},
		{"stop", "Stop Host"}, {"restart", "Restart Host"}, {"update", "Update Host"}, {"help", "Command help"},
	}
	if _, err := fmt.Fprintln(output, "\n"+versionLabel(product)); err != nil {
		return "", err
	}
	for index, item := range commands {
		if _, err := fmt.Fprintf(output, "  %d  %s\n", index+1, item.label); err != nil {
			return "", err
		}
	}
	if _, err := fmt.Fprintln(output, "  0  Exit"); err != nil {
		return "", err
	}
	for {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		if _, err := io.WriteString(output, "\nCommand [1]: "); err != nil {
			return "", err
		}
		value, err := input.ReadString('\n')
		if err != nil {
			return "", err
		}
		value = strings.TrimSpace(value)
		if value == "" {
			value = "1"
		}
		if value == "0" || value == "q" || value == "quit" {
			return "", nil
		}
		for index, item := range commands {
			if value == item.command || value == fmt.Sprint(index+1) {
				return item.command, nil
			}
		}
		if err := ctx.Err(); err != nil {
			return "", err
		}
		if _, err := fmt.Fprintln(output, "Choose a command number, its name, or 0 to exit."); err != nil {
			return "", err
		}
	}
}

func terminal(value any) bool {
	file, ok := value.(*os.File)
	return ok && isatty.IsTerminal(file.Fd())
}

func RunInstalledCommand(ctx context.Context, action string, args []string) error {
	return runInstalledCommand(ctx, action, args, identity.Desktop{}, os.Stdin, os.Stdout, os.Stderr)
}

func runInstalledCommand(ctx context.Context, action string, args []string, product identity.Desktop, stdin io.Reader, stdout, stderr io.Writer) error {
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	name := product.HostRelease.Package().Name()
	launcher := filepath.Join(filepath.Dir(executable), name)
	if info, err := os.Stat(launcher); err != nil || !info.Mode().IsRegular() {
		return fmt.Errorf("managed service commands require an installed Host; use %s serve for a foreground Host", name)
	}
	command := exec.CommandContext(ctx, launcher, append([]string{action}, args...)...)
	command.Stdin, command.Stdout, command.Stderr = stdin, stdout, stderr
	return command.Run()
}
