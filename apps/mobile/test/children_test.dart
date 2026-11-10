import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/children.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  test('resolves only Client navigation targets', () {
    final session = {'id': 'child'};
    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      snapshot: {
        'sessions': [session],
      },
      childTargets: {'captured-target': 'child'},
      command: (_, _) async => {},
    );
    addTearDown(host.dispose);
    expect(childSession(host, {'target': 'captured-target'}), session);
    expect(childSession(host, {'target': 'stale-target'}), isNull);
    host.childTargets = {};
    expect(childSession(host, {'target': 'captured-target'}), isNull);
  });
}
