package main

import (
	"context"
	"crypto/ed25519"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/peerrelay"
)

const usage = `Usage:
  sailry-relay start [--config <path>] [--port <port>]
  sailry-relay start --data-dir <absolute private relay directory> [--port <port> | --listen <multiaddr> [--announce <multiaddr>]] (--public | --allow <PeerID> ...)
  sailry-relay serve --data-dir <absolute private relay directory> --listen <multiaddr> [--announce <multiaddr>] (--public | --allow <PeerID> ...)
  sailry-relay daemon --help
  sailry-relay id --data-dir <absolute private Sailry profile>
  sailry-relay --version

Start reads ./relay.toml by default. With no config file, use --data-dir and an explicit access mode.
Start defaults to TCP on 127.0.0.1:4001. With a config file, --port overrides both TCP ports for this run only.
Select one direct TCP or UDP/QUIC-v1 address. A wildcard listener requires a non-loopback announce address using the same transport.
Public mode admits standard libp2p peers within resource limits. In private mode both peers must be allowed.
Use a separate private data directory for each relay.`

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	err := run(ctx, os.Args[1:], os.Stdout, os.Stderr)
	stop()
	if err != nil {
		fmt.Fprintln(os.Stderr, "sailry-relay:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context, args []string, output, diagnostics io.Writer) error {
	if len(args) == 0 {
		return errors.New("expected a command; use --help")
	}
	command := args[0]
	switch command {
	case "-h", "--help", "help":
		_, err := fmt.Fprintln(output, usage)
		return err
	case "--version":
		_, err := fmt.Fprintln(output, "Sailry Relay", link.ProductVersion)
		return err
	case "start":
		return runStart(ctx, args[1:], output, diagnostics)
	case "daemon":
		return runDaemon(ctx, args[1:], output, diagnostics)
	case "id", "serve":
	default:
		return fmt.Errorf("unknown command %q; use --help", command)
	}

	flags := flag.NewFlagSet("sailry-relay "+command, flag.ContinueOnError)
	flags.SetOutput(diagnostics)
	directory := flags.String("data-dir", "", "Absolute private data directory")
	var cfg peerrelay.Config
	if command == "serve" {
		registerRelayFlags(flags, &cfg)
	}
	if err := flags.Parse(args[1:]); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	if flags.NArg() != 0 || !filepath.IsAbs(*directory) {
		return errors.New("specify an absolute --data-dir and no positional arguments")
	}
	path := filepath.Clean(*directory)
	if command == "id" {
		return writeID(path, output)
	}
	if len(cfg.ListenAddr) == 0 || cfg.Public == (len(cfg.AllowedPeers) != 0) {
		return errors.New("serve requires --listen and exactly one access mode: --public or one or more --allow PeerIDs")
	}
	return serve(ctx, path, cfg, output)
}

func registerRelayFlags(flags *flag.FlagSet, cfg *peerrelay.Config) {
	flags.BoolVar(&cfg.Public, "public", false, "Accept standard libp2p peers within resource limits; incompatible with --allow")
	flags.Func("listen", "Direct /ip4 or /ip6 TCP or UDP/QUIC-v1 multiaddr", multiaddrFlag(&cfg.ListenAddr))
	flags.Func("announce", "Dialable non-loopback multiaddr (required for a wildcard listen address)", multiaddrFlag(&cfg.AnnounceAddr))
	flags.Func("allow", "Authorized PeerID; repeat for each peer", func(value string) error {
		id, err := peer.Decode(value)
		if err != nil {
			return err
		}
		cfg.AllowedPeers = append(cfg.AllowedPeers, id)
		return nil
	})
}

func multiaddrFlag(dst *ma.Multiaddr) func(string) error {
	return func(value string) error {
		if len(*dst) != 0 {
			return errors.New("may only be specified once")
		}
		addr, err := ma.NewMultiaddr(value)
		if err != nil {
			return err
		}
		if len(addr) == 0 {
			return errors.New("multiaddr is required")
		}
		*dst = addr
		return nil
	}
}

func writeID(directory string, output io.Writer) error {
	if _, err := os.Lstat(directory); err != nil {
		return err
	}
	if err := core.PrivateDirectory(directory); err != nil {
		return err
	}
	id, err := readRelayPeerID(directory)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintln(output, id)
	return err
}

func readRelayPeerID(directory string) (peer.ID, error) {
	identity, err := link.ReadIdentity(directory)
	if err != nil {
		return "", fmt.Errorf("read Link identity: %w", err)
	}
	nodeID := identity.ID()
	public, err := crypto.UnmarshalEd25519PublicKey(nodeID[:])
	if err != nil {
		return "", err
	}
	return peer.IDFromPublicKey(public)
}

func serve(ctx context.Context, directory string, cfg peerrelay.Config, output io.Writer) (result error) {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := core.PrivateDirectory(directory); err != nil {
		return err
	}
	identity, err := link.OpenIdentity(directory)
	if err != nil {
		return fmt.Errorf("open relay identity: %w", err)
	}
	seed := identity.SecretKey()
	cfg.PrivateKey, err = crypto.UnmarshalEd25519PrivateKey(ed25519.NewKeyFromSeed(seed[:]))
	if err != nil {
		return err
	}
	service, err := peerrelay.New(cfg)
	if err != nil {
		return err
	}
	defer func() { result = errors.Join(result, service.Close()) }()
	if _, err := fmt.Fprintln(output, service.Addr()); err != nil {
		return fmt.Errorf("print relay address: %w", err)
	}
	<-ctx.Done()
	return nil
}
