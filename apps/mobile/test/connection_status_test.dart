import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/connection_status.dart';

void main() {
  test("separates routes and observed latency through disconnect", () {
    for (final (caseName, run) in <(String, void Function(String))>[
      (
        "selected route and RTT remain separate",
        (caseName) {
          for (final route in ['p2p', 'relay']) {
            final columns = connectionColumns(true, {
              'route': route,
              'rtt_ms': 1248,
            });
            expect(
              columns.$1,
              route == 'p2p' ? 'connectionRouteP2p' : 'connectionRouteRelay',
              reason: caseName,
            );
            expect(columns.$2, '1248 ms', reason: caseName);
          }
        },
      ),
      (
        "disconnect and unknown routes discard stale RTT",
        (caseName) {
          expect(connectionColumns(false, {'route': 'relay', 'rtt_ms': 42}), (
            'connectionRouteUnknown',
            null,
          ), reason: caseName);
          expect(connectionColumns(true, {'rtt_ms': 42}), (
            'connectionRouteUnknown',
            null,
          ), reason: caseName);
          expect(connectionColumns(true, {'route': 'local', 'rtt_ms': 0}), (
            'connectionRouteUnknown',
            null,
          ), reason: caseName);
          expect(connectionColumns(true, {'route': 'p2p', 'rtt_ms': -1}), (
            'connectionRouteP2p',
            null,
          ), reason: caseName);
        },
      ),
    ]) {
      run(caseName);
    }
  });
}
