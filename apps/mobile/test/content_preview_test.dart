import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/paths.dart';

void main() {
  test('file links strip line suffixes without changing literal paths', () {
    for (final source in [
      '/project/src/main.rs:42',
      'src/main.rs#L42',
      'file:///project/src/main.rs:42',
    ]) {
      expect(
        resourcePath(Uri.parse(source), '/project', lineReference: true),
        'src/main.rs',
      );
    }
    for (final source in [
      'src/main.rs:0',
      'src/main.rs#L-1',
      'src/main.rs:3#L4',
      'src/main.rs#L4294967296',
    ]) {
      expect(
        resourcePath(Uri.parse(source), '/project', lineReference: true),
        isNull,
      );
    }
    expect(
      resourcePath(Uri.parse('src/main.rs:42'), '/project'),
      'src/main.rs:42',
    );
    expect(
      resourcePath(
        Uri.parse('file:///C:/project/src/main.rs:42'),
        r'C:\project',
        lineReference: true,
      ),
      'src/main.rs',
    );
  });
  test(
    'Markdown images resolve relative to the document within its worktree',
    () {
      for (final (source, expected) in [
        ('images/a%20b.png', 'docs/images/a b.png'),
        ('../assets/a.png', 'assets/a.png'),
        ('/project/assets/a.png', 'assets/a.png'),
        ('../../outside.png', null),
        ('file:///other/image.png', null),
      ]) {
        expect(
          resourcePath(Uri.parse(source), '/project', directory: 'docs'),
          expected,
        );
      }
    },
  );
}
