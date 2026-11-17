import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/document_images.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  test('document images stay relative to their captured worktree', () {
    for (final (reference, expected) in [
      ('../images/logo.png', 'images/logo.png'),
      ('images/a%20b.svg', 'docs/images/a b.svg'),
      ('../images/%252e%252e.png', 'images/%2e%2e.png'),
      ('../../outside.png', null),
      ('%2e%2e/%2e%2e/outside.png', null),
      ('/images/logo.png', null),
      ('%2fimages/logo.png', null),
      ('file:///project/images/logo.png', null),
      ('//other/images/logo.png', null),
      ('images/a%5cb.png', null),
      ('images/a%3ab.png', null),
      ('images/a%00b.png', null),
    ]) {
      expect(
        documentImagePath('docs/readme.md', Uri.parse(reference)),
        expected,
        reason: reference,
      );
    }
    expect(documentImagePath('../readme.md', Uri.parse('image.png')), isNull);
    expect(documentImagePath('/readme.md', Uri.parse('image.png')), isNull);
  });

  test('document image identity captures entry target and version', () {
    final first = HostConnection.test(id: 'entry-a/target', label: 'Target');
    final other = HostConnection.test(id: 'entry-c/target', label: 'Target');
    final source = DocumentImageSource(
      host: first,
      worktree: 'tree',
      path: 'docs/readme.md',
      version: 'v1',
    );
    expect(source.host, same(first));
    expect(source.host.entryId, 'entry-a');
    expect(source.host.nodeId, 'target');
    expect(source.atVersion('v2').identity, isNot(source.identity));
    expect(
      DocumentImageSource(
        host: other,
        worktree: 'tree',
        path: 'docs/readme.md',
        version: 'v1',
      ).identity,
      isNot(source.identity),
    );
    first.dispose();
    other.dispose();
  });
}
