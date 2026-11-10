import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/terminal_text.dart';

void main() {
  group('plainTerminalText', () {
    test("preserves text while removing terminal control sequences", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (plainTerminalText('a\tb\nc'), 'a\tb\nc', "keeps plain text unchanged"),
        (
          plainTerminalText(
            '\x1B[32m\x1B[1m✓\x1B[22m\x1B[39m Built\x1B]0;t\x07',
          ),
          '✓ Built',
          "removes color and title sequences",
        ),
        (
          plainTerminalText('⠋ a\r⠙ a\r\x1B[Kdone\r\nnext'),
          'done\nnext',
          "keeps the last carriage-return frame",
        ),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });
}
