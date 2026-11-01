import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/terminal_text.dart';

void main() {
  group('plainTerminalText', () {
    test('keeps plain text unchanged', () {
      expect(plainTerminalText('a\tb\nc'), 'a\tb\nc');
    });

    test('removes color and title sequences', () {
      expect(
        plainTerminalText('\x1B[32m\x1B[1m✓\x1B[22m\x1B[39m Built\x1B]0;t\x07'),
        '✓ Built',
      );
    });

    test('keeps the last carriage-return frame', () {
      expect(plainTerminalText('⠋ a\r⠙ a\r\x1B[Kdone\r\nnext'), 'done\nnext');
    });
  });
}
