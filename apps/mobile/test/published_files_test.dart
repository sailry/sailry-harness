import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/files.dart';

Map<String, dynamic> publication(String name, String path, {int size = 100}) =>
    {
      'kind': 'file',
      'name': name,
      'path': path,
      'mime': 'text/markdown',
      'size': size,
      'revision': 'ab12',
    };

Map<String, dynamic> call(String id, Map<String, dynamic> file) => {
  'name': id,
  'turn': 'turn',
  'state': 'returned',
  'presentation': 'content',
  'source': {'entry': id, 'index': 0},
  'response': {'entry': id, 'index': 1},
  'content': {
    'version': 1,
    'blocks': [file],
  },
};

void main() {
  test('retains latest metadata within the same turn', () {
    final first = publication('First', 'first.md');
    final old = publication('Old guide', 'guide.md');
    final latest = publication('Published guide', 'guide.md', size: 2400);
    expect(
      publishedFiles([
        call('first', first),
        call('old', old),
        call('latest', latest),
        {...call('other', old), 'turn': 'another-turn'},
      ], 'turn'),
      [first, latest],
    );
  });
}
