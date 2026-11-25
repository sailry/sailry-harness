import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/paths.dart';

void main() {
  test("split names and keep short directory tails", () {
    expect(splitPath('a/b/c.dart'), (name: 'c.dart', directory: 'a/b'));
    expect(splitPath('c.dart'), (name: 'c.dart', directory: ''));
    expect(shortDirectory('apps/mobile/lib/features'), 'lib/features');
    expect(shortDirectory('lib'), 'lib');
    expect(shortDirectory(''), '');
  });
}
