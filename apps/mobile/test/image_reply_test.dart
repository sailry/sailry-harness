import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/paths.dart';

void main() {
  test('image paths stay within the captured worktree', () {
    expect(resourcePath(Uri.parse('assets/a%20b.png'), null), 'assets/a b.png');
    expect(
      resourcePath(Uri.parse('file:///project/assets/a.png'), '/project'),
      'assets/a.png',
    );
    expect(resourcePath(Uri.parse('/another/a.png'), '/project'), isNull);
    expect(resourcePath(Uri.parse('../a.png'), '/project'), isNull);
    expect(resourcePath(Uri.parse('.'), '/project', allowRoot: true), '');
    expect(
      resourcePath(Uri.parse('/project'), '/project', allowRoot: true),
      '',
    );
    expect(
      resourcePath(Uri.parse('/project-other'), '/project', allowRoot: true),
      isNull,
    );
    expect(resourcePath(Uri.parse('..'), '/project', allowRoot: true), isNull);
    expect(
      resourcePath(
        Uri.parse('file://other/project'),
        '/project',
        allowRoot: true,
      ),
      isNull,
    );
    expect(resourcePath(Uri.parse('.'), '/project'), isNull);
    expect(
      resourcePath(Uri.parse('#heading'), '/project', directory: 'docs'),
      isNull,
    );
    expect(resourcePath(Uri.parse('%FF.md'), '/project'), isNull);
    expect(
      resourcePath(Uri.parse('../guide.md'), '/project', directory: 'docs'),
      'guide.md',
    );
    expect(
      resourcePath(Uri.parse('file:///C:/project/a.png'), r'C:\project'),
      'a.png',
    );
    expect(
      resourcePath(Uri.parse('file://another/project/a.png'), '/project'),
      isNull,
    );
    expect(resourcePath(Uri.parse('assets/../a.png'), '/project'), 'a.png');
  });
}
