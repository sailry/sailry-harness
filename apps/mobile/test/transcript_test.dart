import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/turn_frame.dart';

void main() {
  test('Node duration freezes on completion', () {
    final run = {'status': 'running', 'started_ms': 1000};
    expect(elapsed(run, 13000), '12s');
    expect(elapsed(run, 72000), '1m11s');
    expect(elapsed(run, 3603000), '1h00m02s');
    expect(elapsed(run, 0), '0s');
    expect(
      elapsed({...run, 'status': 'completed', 'finished_ms': 18000}, 999999),
      '17s',
    );
    expect(elapsed({'status': 'queued'}, 99999), isNull);
  });
}
