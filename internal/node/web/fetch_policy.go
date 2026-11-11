// Package web supplies Node-owned public web reads and configured search.
package web

import (
	"context"
	"net"
	"net/netip"
	"net/url"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/idna"
)

func webFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}
func webInvalid(message string) error     { return webFault(protocol.ErrorCodeInvalidRequest, message) }
func webUnavailable(message string) error { return webFault(protocol.ErrorCodeUnavailable, message) }
func addressDenied() error {
	return webFault(protocol.ErrorCodePermissionDenied, "web fetch requires a public network address")
}

func publicURL(value string) (*url.URL, error) {
	parsed, err := url.Parse(value)
	if err != nil || parsed.Scheme != "http" && parsed.Scheme != "https" || parsed.Hostname() == "" || parsed.User != nil && parsed.User.Username() != "" {
		return nil, webInvalid("web URL must use HTTP(S) without credentials")
	}
	if parsed.User != nil {
		if _, ok := parsed.User.Password(); ok {
			return nil, webInvalid("web URL must use HTTP(S) without credentials")
		}
	}
	host := strings.ToLower(parsed.Hostname())
	if ip, err := netip.ParseAddr(host); err == nil {
		if !publicAddress(ip) {
			return nil, addressDenied()
		}
	} else if ip, numeric, valid := numericHost(host); numeric {
		if !valid {
			return nil, webInvalid("invalid web URL")
		}
		if !publicAddress(ip) {
			return nil, addressDenied()
		}
		host = ip.String()
	} else {
		host, err = idna.Lookup.ToASCII(host)
		if err != nil {
			return nil, webInvalid("invalid web URL")
		}
		trimmed := strings.TrimRight(host, ".")
		if trimmed == "localhost" || strings.HasSuffix(trimmed, ".localhost") || strings.HasSuffix(trimmed, ".local") {
			return nil, addressDenied()
		}
	}
	port := parsed.Port()
	if port != "" {
		parsed.Host = net.JoinHostPort(host, port)
	} else if strings.Contains(host, ":") {
		parsed.Host = "[" + host + "]"
	} else {
		parsed.Host = host
	}
	parsed.Fragment, parsed.RawFragment = "", ""
	if parsed.Path == "" {
		parsed.Path = "/"
	}
	return parsed, nil
}

// numericHost reads the IPv4 spellings a URL host may use beside dotted
// decimal: fewer than four parts, hexadecimal and octal, as in http://127.1/
// and http://0x7f000001/. Resolvers and proxies read them as addresses, so
// they are checked as addresses, as a WHATWG URL parser does. numeric is false
// for a host that is a name.
func numericHost(host string) (address netip.Addr, numeric, valid bool) {
	part := func(text string) (uint64, bool) {
		base := 10
		if strings.HasPrefix(text, "0x") {
			text, base = text[2:], 16
			if text == "" {
				return 0, true
			}
		} else if len(text) > 1 && text[0] == '0' {
			text, base = text[1:], 8
		}
		value, err := strconv.ParseUint(text, base, 64)
		return value, err == nil
	}
	parts := strings.Split(strings.TrimSuffix(host, "."), ".")
	last := parts[len(parts)-1]
	if _, number := part(last); last == "" || !number && strings.Trim(last, "0123456789") != "" {
		return netip.Addr{}, false, false
	}
	if len(parts) > 4 {
		return netip.Addr{}, true, false
	}
	var packed uint64
	for index, text := range parts {
		value, number := part(text)
		limit, shift := uint64(255), 8*(3-index)
		if index == len(parts)-1 {
			limit, shift = 1<<(8*(5-len(parts)))-1, 0
		}
		if text == "" || !number || value > limit {
			return netip.Addr{}, true, false
		}
		packed |= value << shift
	}
	return netip.AddrFrom4([4]byte{byte(packed >> 24), byte(packed >> 16), byte(packed >> 8), byte(packed)}), true, true
}

func publicAddress(ip netip.Addr) bool {
	ip = ip.Unmap()
	if ip.Is4() {
		v := ip.As4()
		a, b, c := v[0], v[1], v[2]
		return !(ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() || ip.IsMulticast() || a == 0 || a >= 240 || a == 100 && b >= 64 && b <= 127 || a == 192 && b == 0 && (c == 0 || c == 2) || a == 192 && b == 88 && c == 99 || a == 198 && (b == 18 || b == 19) || a == 198 && b == 51 && c == 100 || a == 203 && b == 0 && c == 113)
	}
	if !ip.Is6() {
		return false
	}
	v := ip.As16()
	first, second := uint16(v[0])<<8|uint16(v[1]), uint16(v[2])<<8|uint16(v[3])
	return first&0xe000 == 0x2000 && !(first == 0x2001 && second < 0x0200) && !(first == 0x2001 && second == 0x0db8) && first != 0x2002 && !(first == 0x3fff && second&0xf000 == 0)
}

func checkAddresses(addresses, local []netip.Addr) error {
	if len(addresses) == 0 {
		return addressDenied()
	}
	for _, address := range addresses {
		if !publicAddress(address) {
			return addressDenied()
		}
		for _, own := range local {
			if address.Unmap() == own.Unmap() {
				return addressDenied()
			}
		}
	}
	return nil
}

func resolvePublic(ctx context.Context, host string) ([]netip.Addr, error) {
	if address, err := netip.ParseAddr(strings.Trim(host, "[]")); err == nil {
		return []netip.Addr{address}, nil
	}
	addresses, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
	if err != nil {
		return nil, webUnavailable("web hostname could not be resolved")
	}
	return addresses, nil
}

func localAddresses() ([]netip.Addr, error) {
	addresses, err := net.InterfaceAddrs()
	if err != nil {
		return nil, webUnavailable("local network addresses are unavailable")
	}
	result := make([]netip.Addr, 0, len(addresses))
	for _, address := range addresses {
		if prefix, err := netip.ParsePrefix(address.String()); err == nil {
			result = append(result, prefix.Addr())
		}
	}
	return result, nil
}
