import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/background_data.dart';

void main() {
  test('background state preserves user input and pending reservations', () {
    expect(noticeTurn({}), isFalse);
    expect(noticeTurn({'notice': true}), isTrue);
    expect(pendingBackground({}), isEmpty);
    final ready = {
      'id': 'ready',
      'state': 'pending',
      'delivery': null,
      'outcome': 'completed',
      'recovered': true,
    };
    expect(
      pendingBackground({
        'background': [
          ready,
          {
            'id': 'reserved',
            'state': 'pending',
            'delivery': 'turn',
            'outcome': 'completed',
          },
          {'id': 'running', 'state': 'running'},
          {'id': 'delivered', 'state': 'delivered', 'outcome': 'completed'},
        ],
      }),
      [ready],
    );
    expect(backgroundLabel('unknown'), 'backgroundUnknown');
    expect(backgroundLabel('timed_out'), 'backgroundTimedOut');
  });
}
