package link

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/netip"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"sync"

	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/assets/catalogs"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type relayInputDNS interface {
	LookupTXT(context.Context, string) ([]string, error)
	LookupNetIP(context.Context, string, string) ([]netip.Addr, error)
}

var bundledRelayPresets = sync.OnceValue(func() []protocol.NetworkPreset {
	var catalog protocol.NetworkCatalog
	if json.Unmarshal([]byte(catalogs.NetworkPresetsJSON()), &catalog) != nil {
		return nil
	}
	return catalog.Presets
})

// ResolveNetworkSelection resolves user input before a selection is applied or
// persisted. Stored selections and transport addresses never depend on DNSADDR
// to recover the relay's identity.
func ResolveNetworkSelection(ctx context.Context, selection protocol.NetworkSelection, presets []protocol.NetworkPreset) (protocol.NetworkSelection, error) {
	return resolveNetworkSelection(ctx, selection, presets, net.DefaultResolver)
}

func resolveNetworkSelection(ctx context.Context, selection protocol.NetworkSelection, presets []protocol.NetworkPreset, dns relayInputDNS) (protocol.NetworkSelection, error) {
	_, invalid := NetworkRelayURLs(ctx, selection)
	if invalid == nil {
		return selection, nil
	}
	relays := customRelayAddrs(selection.Data)
	if selection.Kind != "custom" || len(relays) == 0 || len(relays) > 2 {
		return protocol.NetworkSelection{}, invalid
	}
	relays = slices.Clone(relays)
	for i, value := range relays {
		var err error
		value = strings.TrimSpace(value)
		if strings.HasPrefix(value, "/") {
			relays[i], err = canonicalLibp2pRelay(value)
		} else {
			relays[i], err = resolveRelayInput(ctx, value, presets, dns)
		}
		if err != nil {
			return protocol.NetworkSelection{}, err
		}
	}
	selection.Data = protocol.NetworkSelectionCustomPayload{Relays: relays}
	if _, err := NetworkRelayURLs(ctx, selection); err != nil {
		return protocol.NetworkSelection{}, err
	}
	return selection, nil
}

type relayInput struct {
	host string
	ip   netip.Addr
	port string
}

func parseRelayInput(value string) (relayInput, error) {
	invalid := invalidLink("enter a relay hostname or IP with an optional port, or a libp2p relay multiaddr")
	if strings.Contains(value, "://") {
		origin, err := url.Parse(value)
		if err != nil || origin.Scheme != "https" || origin.User != nil || strings.ContainsAny(value, "?#") ||
			origin.EscapedPath() != "" && origin.EscapedPath() != "/" {
			return relayInput{}, invalid
		}
		value = origin.Host
	}
	host, port := value, ""
	if _, err := netip.ParseAddr(value); err != nil && strings.Contains(value, ":") {
		host, port, err = net.SplitHostPort(value)
		number, portErr := strconv.ParseUint(port, 10, 16)
		if err != nil || portErr != nil || number == 0 {
			return relayInput{}, invalid
		}
		port = strconv.FormatUint(number, 10)
	}
	if ip, err := netip.ParseAddr(host); err == nil {
		if ip.Zone() != "" || ip.IsUnspecified() || ip.IsMulticast() {
			return relayInput{}, invalid
		}
		return relayInput{ip: ip.Unmap(), port: port}, nil
	}
	host = strings.ToLower(strings.TrimSuffix(host, "."))
	if len(host) > 253 || !strings.Contains(host, ".") {
		return relayInput{}, invalid
	}
	for label := range strings.SplitSeq(host, ".") {
		if len(label) == 0 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' || strings.ContainsFunc(label, func(c rune) bool {
			return !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-')
		}) {
			return relayInput{}, invalid
		}
	}
	return relayInput{host: host, port: port}, nil
}

func resolveRelayInput(ctx context.Context, value string, presets []protocol.NetworkPreset, dns relayInputDNS) (string, error) {
	input, err := parseRelayInput(value)
	if err != nil {
		return "", err
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	matches := func(address ma.Multiaddr) bool {
		host := address[0].Value()
		if input.ip.IsValid() {
			ip, err := netip.ParseAddr(host)
			return err == nil && ip.Unmap() == input.ip
		}
		return strings.EqualFold(strings.TrimSuffix(host, "."), input.host)
	}
	for _, catalog := range [][]protocol.NetworkPreset{presets, bundledRelayPresets()} {
		if address, err := matchingRelayPreset(catalog, input.port, matches); address != "" || err != nil {
			return address, err
		}
	}
	if input.ip.IsValid() {
		// An IP alias can select a bundled DNS relay without learning or changing
		// its PeerID. Do not reverse-resolve arbitrary addresses or scan a catalog.
		address, err := matchingRelayPreset(bundledRelayPresets(), input.port, func(address ma.Multiaddr) bool {
			family := "ip"
			switch address[0].Code() {
			case ma.P_DNS4:
				family = "ip4"
			case ma.P_DNS6:
				family = "ip6"
			case ma.P_DNS:
			default:
				return false
			}
			ips, err := dns.LookupNetIP(ctx, family, strings.TrimSuffix(address[0].Value(), ".")+".")
			return err == nil && slices.ContainsFunc(ips, func(ip netip.Addr) bool { return ip.Unmap() == input.ip })
		})
		if err != nil || address != "" {
			return address, err
		}
		if err := ctx.Err(); err != nil {
			return "", err
		}
		return "", invalidLink("no known relay matches this IP and port; use a relay hostname or a peer-addressed multiaddr")
	}
	records, err := dns.LookupTXT(ctx, "_dnsaddr."+input.host+".")
	if err != nil {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		var problem *net.DNSError
		if !errors.As(err, &problem) || !problem.IsNotFound {
			return "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "relay DNS records could not be read"}
		}
	}
	var chosen string
	for _, record := range records {
		value, ok := strings.CutPrefix(record, "dnsaddr=")
		if !ok {
			continue
		}
		spec, err := parseLibp2pRelay(value)
		if err != nil || input.port != "" && spec.info.Addrs[0][1].Value() != input.port {
			continue
		}
		address := spec.circuit[:len(spec.circuit)-1].String()
		if chosen != "" && chosen != address {
			return "", invalidLink("relay hostname matches multiple libp2p addresses; specify a port or use a multiaddr")
		}
		chosen = address
	}
	if chosen == "" {
		return "", invalidLink("relay hostname has no peer-addressed _dnsaddr record for this port")
	}
	return chosen, nil
}

func matchingRelayPreset(presets []protocol.NetworkPreset, port string, matches func(ma.Multiaddr) bool) (string, error) {
	var chosen string
	for _, preset := range presets {
		if preset.Selection.Kind != "custom" {
			continue
		}
		for _, value := range customRelayAddrs(preset.Selection.Data) {
			spec, err := parseLibp2pRelay(value)
			if err != nil {
				continue
			}
			base := spec.info.Addrs[0]
			if port != "" && base[1].Value() != port || !matches(base) {
				continue
			}
			address := spec.circuit[:len(spec.circuit)-1].String()
			if chosen != "" && chosen != address {
				return "", invalidLink("relay address matches multiple server presets; specify a port or use a multiaddr")
			}
			chosen = address
		}
	}
	return chosen, nil
}
