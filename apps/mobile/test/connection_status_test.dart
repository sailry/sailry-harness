import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/connection_status.dart';

void main() {
  test('selected route and RTT remain separate', () {
    for (final route in ['p2p', 'relay']) {
      final columns = connectionColumns(true, {'route': route, 'rtt_ms': 1248});
      expect(
        columns.$1,
        route == 'p2p' ? 'connectionRouteP2p' : 'connectionRouteRelay',
      );
      expect(columns.$2, '1248 ms');
    }
  });
  test('disconnect and unknown routes discard stale RTT', () {
    expect(connectionColumns(false, {'route': 'relay', 'rtt_ms': 42}), (
      'connectionRouteUnknown',
      null,
    ));
    expect(connectionColumns(true, {'rtt_ms': 42}), (
      'connectionRouteUnknown',
      null,
    ));
    expect(connectionColumns(true, {'route': 'local', 'rtt_ms': 0}), (
      'connectionRouteUnknown',
      null,
    ));
    expect(connectionColumns(true, {'route': 'p2p', 'rtt_ms': -1}), (
      'connectionRouteP2p',
      null,
    ));
  });
}
