import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';

void main() {
  test('citation offsets preserve Unicode and part boundaries', () {
    final citations = [
      {'uri': 'https://example.com/a_(b)', 'end': 3},
      {'uri': 'https://example.com/end'},
    ];
    expect(citedText('A😀', citations, 0, false), 'A😀');
    expect(
      citedText('B', citations, 2, true),
      'B [1](<https://example.com/a_(b)>) [2](<https://example.com/end>)',
    );
    expect(
      citedText(
        'Raw',
        [
          {'uri': 'javascript:alert(1)', 'end': 3},
        ],
        0,
        true,
      ),
      'Raw',
    );
  });
}
