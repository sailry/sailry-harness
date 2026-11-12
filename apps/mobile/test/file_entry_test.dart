import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/files/entry_data.dart';

void main() {
  test('an entry states its modification time only when the host did', () {
    expect(
      entryModified({'name': 'a', 'modified_ms': 1700000000000}),
      DateTime.fromMillisecondsSinceEpoch(1700000000000),
    );
    for (final value in <Object?>[null, 0, -1, 'yesterday']) {
      expect(entryModified({'modified_ms': value}), isNull, reason: '$value');
    }
    expect(entryModified({}), isNull);
  });

  test('a location is drawn by its kind and a device by its medium', () {
    for (final (location, icon) in <(Map<String, dynamic>, String)>[
      ({'kind': 'home'}, 'user'),
      ({'kind': 'desktop'}, 'laptop'),
      ({'kind': 'documents'}, 'file'),
      ({'kind': 'downloads'}, 'arrow-down'),
      ({'kind': 'applications'}, 'grid'),
      ({'kind': 'volume'}, 'database'),
      ({'kind': 'volume', 'medium': 'internal'}, 'database'),
      ({'kind': 'volume', 'medium': 'external'}, 'archive'),
      ({'kind': 'volume', 'medium': 'network'}, 'globe'),
      ({'kind': 'volume', 'medium': 'later'}, 'database'),
      ({'kind': 'root', 'medium': 'network'}, 'globe'),
      // The medium belongs to a device; a folder kind ignores it.
      ({'kind': 'home', 'medium': 'network'}, 'user'),
      ({'kind': 'later'}, 'folder'),
      ({}, 'folder'),
    ]) {
      expect(locationIcon(location), icon, reason: '$location');
    }
  });
}
