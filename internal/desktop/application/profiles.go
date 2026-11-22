package application

import (
	"log"
	"net"
	"net/http"
	"net/http/pprof"
	"os"
)

// serveProfiles serves the Go runtime's profiles (net/http/pprof) on the
// loopback address SAILRY_PPROF names, for development runs started by
// `just desktop`. Without it nothing listens; another host is refused, since
// profiles expose the process's memory.
func serveProfiles() {
	address := os.Getenv("SAILRY_PPROF")
	if address == "" {
		return
	}
	host, _, err := net.SplitHostPort(address)
	if ip := net.ParseIP(host); err != nil || ip == nil || !ip.IsLoopback() {
		log.Printf("profiles: %q is not a loopback address", address)
		return
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/debug/pprof/", pprof.Index)
	mux.HandleFunc("/debug/pprof/cmdline", pprof.Cmdline)
	mux.HandleFunc("/debug/pprof/profile", pprof.Profile)
	mux.HandleFunc("/debug/pprof/symbol", pprof.Symbol)
	mux.HandleFunc("/debug/pprof/trace", pprof.Trace)
	listener, err := net.Listen("tcp", address)
	if err != nil {
		log.Printf("profiles: %v", err)
		return
	}
	log.Printf("profiles: http://%s/debug/pprof/", listener.Addr())
	go func() { _ = http.Serve(listener, mux) }()
}
