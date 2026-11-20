package main

import (
	"bytes"
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"maps"
	"net/netip"
	"os"
	"path/filepath"
	"slices"
	"strconv"

	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"
	"github.com/pelletier/go-toml/v2"
	"github.com/sailry/sailry-harness/internal/link/peerrelay"
)

const (
	relayConfigName      = "relay.toml"
	maxRelayConfig       = 64 << 10
	defaultRelayListenIP = "127.0.0.1"
	defaultRelayPort     = 4001
)

type startOptions struct {
	directory string
	config    peerrelay.Config
}

func runStart(ctx context.Context, args []string, output, diagnostics io.Writer) error {
	options, err := parseStartOptions(args, diagnostics)
	if errors.Is(err, flag.ErrHelp) {
		return nil
	}
	if err != nil {
		return err
	}
	return serve(ctx, options.directory, options.config, output)
}

func parseStartOptions(args []string, diagnostics io.Writer) (startOptions, error) {
	flags := flag.NewFlagSet("sailry-relay start", flag.ContinueOnError)
	flags.SetOutput(diagnostics)
	path := flags.String("config", relayConfigName, "Path to a regular relay TOML file (no symlinks)")
	port := flags.Int64("port", defaultRelayPort, "TCP listen and announce port; incompatible with --listen")
	directory := flags.String("data-dir", "", "Absolute private data directory (without a config file)")
	var cfg peerrelay.Config
	registerRelayFlags(flags, &cfg)
	if err := flags.Parse(args); err != nil {
		return startOptions{}, err
	}
	if flags.NArg() != 0 {
		return startOptions{}, errors.New("start does not accept positional arguments")
	}
	if *path == "" {
		return startOptions{}, errors.New("specify a non-empty --config path")
	}
	var useConfig, overridePort bool
	var serveFlag string
	flags.Visit(func(f *flag.Flag) {
		switch f.Name {
		case "config":
			useConfig = true
		case "port":
			overridePort = true
		default:
			if serveFlag == "" {
				serveFlag = f.Name
			}
		}
	})
	if overridePort && len(cfg.ListenAddr) != 0 {
		return startOptions{}, errors.New("--port cannot be combined with --listen")
	}
	if !useConfig {
		_, err := os.Lstat(*path)
		if err == nil {
			useConfig = true
		} else if !errors.Is(err, os.ErrNotExist) {
			return startOptions{}, fmt.Errorf("check relay config %q: %w", *path, err)
		}
	}
	if useConfig {
		if serveFlag != "" {
			return startOptions{}, fmt.Errorf("--%s cannot be combined with relay config %q", serveFlag, *path)
		}
		options, _, err := readRelayConfig(*path)
		if err != nil || !overridePort {
			return options, err
		}
		tcp, err := relayTCPPort(*port)
		if err != nil {
			return startOptions{}, err
		}
		options.config.ListenAddr[1] = *tcp
		if len(options.config.AnnounceAddr) != 0 {
			options.config.AnnounceAddr[1] = *tcp
		}
		return options, nil
	}
	if !filepath.IsAbs(*directory) {
		return startOptions{}, errors.New("start without a config file requires an absolute --data-dir")
	}
	if cfg.Public == (len(cfg.AllowedPeers) != 0) {
		return startOptions{}, errors.New("start requires exactly one access mode: --public or one or more --allow PeerIDs")
	}
	if len(cfg.ListenAddr) == 0 {
		tcp, err := relayTCPPort(*port)
		if err != nil {
			return startOptions{}, err
		}
		cfg.ListenAddr, err = relayTCPAddress(defaultRelayListenIP, tcp)
		if err != nil {
			return startOptions{}, err
		}
	}
	if err := peerrelay.ValidateConfig(cfg); err != nil {
		return startOptions{}, err
	}
	return startOptions{directory: filepath.Clean(*directory), config: cfg}, nil
}

func readRelayConfig(path string) (startOptions, []byte, error) {
	file, info, err := openRelayRegularFile(path, maxRelayConfig)
	if err != nil {
		return startOptions{}, nil, fmt.Errorf("read relay config %q: %w", path, err)
	}
	defer file.Close()
	var contents bytes.Buffer
	if err := copyRelayRegularFile(&contents, file, info.Size()); err != nil {
		return startOptions{}, nil, fmt.Errorf("read relay config %q: %w", path, err)
	}
	data := contents.Bytes()
	options, err := decodeRelayConfig(data)
	if err != nil {
		return startOptions{}, nil, fmt.Errorf("relay config %q: %w", path, err)
	}
	return options, data, nil
}

func decodeRelayConfig(data []byte) (startOptions, error) {
	var fields map[string]any
	if err := toml.Unmarshal(data, &fields); err != nil {
		return startOptions{}, err
	}
	var mode, directory, announceIP string
	listenIP, port := defaultRelayListenIP, int64(defaultRelayPort)
	var peers []any
	if err := errors.Join(
		takeRelayField(fields, "mode", &mode),
		takeRelayField(fields, "listen_ip", &listenIP),
		takeRelayField(fields, "port", &port),
		takeRelayField(fields, "announce_ip", &announceIP),
		takeRelayField(fields, "data_dir", &directory),
		takeRelayField(fields, "allowed_peers", &peers),
	); err != nil {
		return startOptions{}, err
	}
	if len(fields) != 0 {
		return startOptions{}, fmt.Errorf("unknown relay config fields: %q", slices.Sorted(maps.Keys(fields)))
	}
	if !filepath.IsAbs(directory) {
		return startOptions{}, errors.New("data_dir must be absolute")
	}
	var cfg peerrelay.Config
	switch mode {
	case "public":
		cfg.Public = true
	case "private":
	default:
		return startOptions{}, errors.New(`mode must be "public" or "private"`)
	}
	tcp, err := relayTCPPort(port)
	if err != nil {
		return startOptions{}, err
	}
	cfg.ListenAddr, err = relayTCPAddress(listenIP, tcp)
	if err != nil {
		return startOptions{}, fmt.Errorf("listen_ip: %w", err)
	}
	if announceIP != "" {
		cfg.AnnounceAddr, err = relayTCPAddress(announceIP, tcp)
		if err != nil {
			return startOptions{}, fmt.Errorf("announce_ip: %w", err)
		}
	}
	for i, value := range peers {
		text, ok := value.(string)
		if !ok {
			return startOptions{}, fmt.Errorf("allowed_peers entry %d must be a PeerID string", i+1)
		}
		id, err := peer.Decode(text)
		if err != nil {
			return startOptions{}, fmt.Errorf("invalid allowed_peers entry %d: %w", i+1, err)
		}
		cfg.AllowedPeers = append(cfg.AllowedPeers, id)
	}
	if err := peerrelay.ValidateConfig(cfg); err != nil {
		return startOptions{}, err
	}
	return startOptions{directory: filepath.Clean(directory), config: cfg}, nil
}

func takeRelayField[T any](fields map[string]any, name string, destination *T) error {
	value, present := fields[name]
	if !present {
		return nil
	}
	decoded, ok := value.(T)
	if !ok {
		return fmt.Errorf("%s has the wrong TOML type", name)
	}
	delete(fields, name)
	*destination = decoded
	return nil
}

func relayTCPPort(port int64) (*ma.Component, error) {
	if port < 1 || port > 65535 {
		return nil, errors.New("port must be between 1 and 65535")
	}
	return ma.NewComponent("tcp", strconv.FormatInt(port, 10))
}

func relayTCPAddress(text string, port *ma.Component) (ma.Multiaddr, error) {
	ip, err := netip.ParseAddr(text)
	if err != nil || ip.Zone() != "" {
		return nil, fmt.Errorf("expected an IPv4 or IPv6 address, got %q", text)
	}
	ip = ip.Unmap()
	family := "ip4"
	if ip.Is6() {
		family = "ip6"
	}
	addr, err := ma.NewComponent(family, ip.String())
	if err != nil {
		return nil, err
	}
	return ma.Multiaddr{*addr, *port}, nil
}
