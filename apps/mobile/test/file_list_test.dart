import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/file_size.dart';

void main() {
  test('file sizes handle missing values and unit boundaries', () {
    for (final (bytes, expected) in <(int?, String)>[
      (null, '—'),
      (-1, '—'),
      (0, '0 B'),
      (999, '999 B'),
      (1000, '1 KB'),
      (1536, '1.5 KB'),
      (999950, '1 MB'),
      (2500000, '2.5 MB'),
      (1000000000, '1 GB'),
      (1000000000000, '1 TB'),
    ]) {
      expect(fileSize(bytes), expected, reason: '$bytes bytes');
    }
  });
}
