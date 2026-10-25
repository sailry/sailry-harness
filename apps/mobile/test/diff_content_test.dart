import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/diff.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';

void main() {
  test('diff positions preserve hunk ranges and metadata', () {
    final rows = parseLines(
      '--- a/test.rs\n+++ b/test.rs\n@@ -4,2 +8,2 @@\n-old\n+new\n same\n\\ No newline at end of file',
    );
    expect(rows[1].kind, 'header');
    expect((rows[3].before, rows[3].after), (4, null));
    expect((rows[4].before, rows[4].after), (null, 8));
    expect((rows[5].before, rows[5].after), (5, 9));
    expect(rows[6].before, isNull);
  });
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
