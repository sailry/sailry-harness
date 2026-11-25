import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/diff.dart';

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

  group('hunkLines', () {
    test("preserves hunk numbering metadata and file modes", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "drops file headers and keeps hunk numbering",
          (caseName) {
            final rows = hunkLines(
              'diff --git a/x b/x\nindex 1..2\n--- a/x\n+++ b/x\n'
              '@@ -4,2 +4,2 @@\n-old\n+new\n same\n\\ No newline at end of file',
            );
            expect(rows.map((row) => row.kind), [
              'header',
              'removed',
              'added',
              'context',
            ], reason: caseName);
            expect((rows[1].before, rows[2].after), (4, 4), reason: caseName);
          },
        ),
        (
          "retains change metadata without hunks",
          (caseName) {
            for (final (patch, expected) in [
              (
                'diff --git a/run b/run\nold mode 100644\nnew mode 100755\n',
                ['old mode 100644', 'new mode 100755'],
              ),
              (
                'diff --git a/image.png b/image.png\nindex 123..456 100644\n'
                    'Binary files a/image.png and b/image.png differ\n',
                ['Binary files a/image.png and b/image.png differ'],
              ),
              (
                'diff --git a/empty b/empty\nnew file mode 100644\n'
                    'index 0000000..e69de29\n',
                ['new file mode 100644'],
              ),
              (
                'diff --git a/empty b/empty\ndeleted file mode 100644\n'
                    'index e69de29..0000000\n',
                ['deleted file mode 100644'],
              ),
              (
                'diff --git a/old b/new\nsimilarity index 100%\n'
                    'rename from old\nrename to new\n',
                ['similarity index 100%', 'rename from old', 'rename to new'],
              ),
            ]) {
              final rows = hunkLines(patch);
              expect(
                rows.map((row) => row.text),
                expected,
                reason: '$caseName: $patch',
              );
              expect(
                rows.every((row) => row.kind == 'header'),
                isTrue,
                reason: caseName,
              );
              expect(
                rows.every((row) => row.before == null && row.after == null),
                isTrue,
                reason: caseName,
              );
            }
          },
        ),
        (
          "retains modes alongside numbered content changes",
          (caseName) {
            final rows = hunkLines(
              'diff --git a/run b/run\nold mode 100644\nnew mode 100755\n'
              'index 123..456\n--- a/run\n+++ b/run\n'
              '@@ -4 +8 @@\n-old\n+new\n',
            );
            expect(rows.map((row) => row.text), [
              'old mode 100644',
              'new mode 100755',
              '@@ -4 +8 @@',
              '-old',
              '+new',
            ], reason: caseName);
            expect(rows.map((row) => row.kind), [
              'header',
              'header',
              'header',
              'removed',
              'added',
            ], reason: caseName);
            expect((rows[3].before, rows[4].after), (4, 8), reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });
}
