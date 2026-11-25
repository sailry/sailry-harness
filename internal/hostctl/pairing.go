package hostctl

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type shareRequest struct {
	Version uint8  `json:"version"`
	Command string `json:"command"`
	Origin  string `json:"origin"`
}

type shareResponse struct {
	Version     uint8  `json:"version"`
	State       string `json:"state"`
	Code        string `json:"code,omitempty"`
	ExpiresAtMS uint64 `json:"expires_at_ms,omitempty"`
}

func serveShare(ctx context.Context, line []byte, writer io.Writer, endpoint *link.Link, transport link.Transport) error {
	var request shareRequest
	if len(line) > 8192 || protocol.DecodeObject(line, &request, []string{"version", "command", "origin"}, true) != nil || request.Version != 1 || request.Command != "share" {
		return errors.New("unsupported Host share request")
	}
	session, err := endpoint.Share(ctx, request.Origin)
	if err != nil {
		return err
	}
	defer session.Close()
	for {
		state, err := session.Next(ctx)
		if err != nil {
			return err
		}
		response := shareResponse{Version: 1, State: state.Kind}
		if state.Kind == "ready" {
			response.Code, response.ExpiresAtMS = state.Code, state.ExpiresAtMS
		}
		if err := writeLine(writer, response, 8192); err != nil {
			return err
		}
		if state.Kind == "paired" || state.Kind == "closed" {
			return nil
		}
	}
}

func RunShare(ctx context.Context, args []string, stdout, stderr io.Writer) error {
	return runShare(ctx, args, stdout, stderr, "")
}

func runShare(ctx context.Context, args []string, stdout, stderr io.Writer, defaultProfile string) error {
	profile, origin := "", link.DefaultPairingService
	seen := map[string]bool{}
	for index := 0; index < len(args); index++ {
		flag := args[index]
		if seen[flag] || flag != "--data-dir" && flag != "--pairing-service" || index+1 == len(args) {
			return errors.New("Usage: sailry-host share [--data-dir <profile>] [--pairing-service <HTTPS origin>]")
		}
		seen[flag] = true
		index++
		if flag == "--data-dir" {
			profile = args[index]
			if !filepath.IsAbs(profile) {
				return errors.New("--data-dir must be absolute")
			}
		} else if flag == "--pairing-service" {
			origin = args[index]
		}
	}
	if err := validateOrigin(origin); err != nil {
		return err
	}
	if profile == "" {
		var err error
		profile, err = resolveProfile(defaultProfile)
		if err != nil {
			return err
		}
	}
	connection, err := connectControl(ctx, profile)
	if err != nil {
		return errors.New("Host is not running")
	}
	defer connection.Close()
	stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stop()
	if err := writeLine(connection, shareRequest{Version: 1, Command: "share", Origin: origin}, 8192); err != nil {
		return err
	}
	reader := bufio.NewReader(connection)
	for {
		line, err := readLine(reader, 8192)
		if ctx.Err() != nil {
			return nil
		}
		if err != nil {
			return err
		}
		var response shareResponse
		if protocol.DecodeObject(line, &response, []string{"version", "state"}, true) != nil || response.Version != 1 {
			return errors.New("invalid Host share response")
		}
		state := link.ShareState{Kind: response.State, Code: response.Code, ExpiresAtMS: response.ExpiresAtMS}
		if err := reportShare(state, stdout, stderr); err != nil {
			return err
		}
		if state.Kind == "paired" || state.Kind == "closed" {
			return nil
		}
	}
}

func ReportSharing(ctx context.Context, endpoint *link.Link, origin string, stdout, stderr io.Writer) error {
	session, err := endpoint.Share(ctx, origin)
	if err != nil {
		return err
	}
	defer session.Close()
	for {
		state, err := session.Next(ctx)
		if err != nil {
			if ctx.Err() != nil {
				return nil
			}
			return err
		}
		if err := reportShare(state, stdout, stderr); err != nil {
			return err
		}
		if state.Kind == "paired" || state.Kind == "closed" {
			return nil
		}
	}
}

func reportShare(state link.ShareState, stdout, stderr io.Writer) error {
	var err error
	tty := terminal(stdout)
	switch state.Kind {
	case "preparing":
	case "ready":
		if len(state.Code) != 6 || state.ExpiresAtMS == 0 {
			return errors.New("invalid Host pairing PIN")
		}
		for _, char := range state.Code {
			if char < '0' || char > '9' {
				return errors.New("invalid Host pairing PIN")
			}
		}
		if state.ExpiresAtMS > uint64(1<<63-1) {
			return errors.New("invalid Host pairing expiry")
		}
		remaining := time.Until(time.UnixMilli(int64(state.ExpiresAtMS))).Round(time.Second)
		expiry := "expired"
		if remaining > 0 {
			expiry = "expires in " + humanDuration(remaining)
		}
		terminator := "\n"
		prefix := ""
		if tty {
			prefix, terminator = "\r\033[2K", ""
		}
		_, err = fmt.Fprintf(stdout, "%sPairing code: %s (%s; keep private)%s", prefix, state.Code, expiry, terminator)
	case "retrying":
		if tty {
			_, err = io.WriteString(stdout, "\r\033[2KPairing service unavailable; retrying...")
		} else {
			_, err = fmt.Fprintln(stderr, "Pairing service unavailable; retrying")
		}
	case "paired":
		if tty {
			if _, err = io.WriteString(stdout, "\n"); err != nil {
				return err
			}
		}
		_, err = fmt.Fprintln(stdout, "Pairing complete; sharing stopped")
	case "closed":
		if tty {
			if _, err = io.WriteString(stdout, "\n"); err != nil {
				return err
			}
		}
		_, err = fmt.Fprintln(stdout, "Pairing closed")
	default:
		err = errors.New("invalid Host share state")
	}
	return err
}

func humanDuration(value time.Duration) string {
	seconds := int64(value.Round(time.Second) / time.Second)
	if seconds < 60 {
		return fmt.Sprintf("%ds", seconds)
	}
	minutes, remainder := seconds/60, seconds%60
	if remainder == 0 {
		return fmt.Sprintf("%dm", minutes)
	}
	return fmt.Sprintf("%dm %ds", minutes, remainder)
}

func Bootstrap(ctx context.Context, profile string, endpoint *link.Link) error {
	peers, err := endpoint.Peers(ctx)
	if err != nil || len(peers) > 0 {
		return err
	}
	scoped, cancel := context.WithTimeout(ctx, 600*time.Second)
	defer cancel()
	path := filepath.Join(profile, "bootstrap.ticket")
	for scoped.Err() == nil {
		invitation, err := endpoint.Invite(scoped)
		if err != nil {
			return err
		}
		identity, err := publishTicket(path, invitation.Ticket())
		if err != nil {
			_ = invitation.Close()
			return err
		}
		err = invitation.Paired(scoped)
		_ = invitation.Close()
		removeOwned(path, identity)
		if err == nil || scoped.Err() != nil {
			return nil
		}
	}
	return nil
}

func publishTicket(path, ticket string) (os.FileInfo, error) {
	if metadata, err := os.Lstat(path); err == nil {
		if !metadata.Mode().IsRegular() {
			return nil, errors.New("Host bootstrap path is not a private regular file")
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	file, err := os.CreateTemp(filepath.Dir(path), ".bootstrap-ticket-")
	if err != nil {
		return nil, err
	}
	temporary := file.Name()
	defer os.Remove(temporary)
	if _, err := io.WriteString(file, ticket); err != nil {
		_ = file.Close()
		return nil, err
	}
	if err := errors.Join(file.Sync(), file.Close()); err != nil {
		return nil, err
	}
	if err := os.Rename(temporary, path); err != nil {
		return nil, err
	}
	identity, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	directory, err := os.Open(filepath.Dir(path))
	if err == nil {
		err = errors.Join(directory.Sync(), directory.Close())
	}
	if err != nil {
		removeOwned(path, identity)
		return nil, err
	}
	return identity, nil
}

func removeOwned(path string, identity os.FileInfo) {
	if metadata, err := os.Lstat(path); err == nil && os.SameFile(metadata, identity) {
		_ = os.Remove(path)
	}
}
