import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/diff.dart';
import 'package:sailry_mobile/content/paths.dart';
import 'package:sailry_mobile/features/resources/files/breadcrumb.dart';
import 'package:sailry_mobile/features/resources/git/model.dart';

Map<String, dynamic> _entry(
  String path, {
  String? staged,
  String? unstaged,
  bool untracked = false,
  bool conflicted = false,
}) => {
  'path': path,
  'staged': staged,
  'unstaged': unstaged,
  'untracked': untracked,
  'conflicted': conflicted,
  'diff': {'additions': 5, 'deletions': 2},
  'staged_diff': {'additions': 3, 'deletions': 0},
  'unstaged_diff': {'additions': 2, 'deletions': 2},
};

void main() {
  group('changeStatus', () {
    test("maps staged unstaged untracked and conflicted statuses", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "maps each side to its letter",
          (caseName) {
            final entry = _entry('a', staged: 'added', unstaged: 'modified');
            expect(
              changeStatus(entry, staged: true),
              ChangeStatus.added,
              reason: caseName,
            );
            expect(
              changeStatus(entry, staged: false),
              ChangeStatus.modified,
              reason: caseName,
            );
            expect(
              changeStatus(
                _entry('b', unstaged: 'deleted'),
                staged: false,
              ).letter,
              'D',
              reason: caseName,
            );
          },
        ),
        (
          "marks untracked and conflicted entries",
          (caseName) {
            expect(
              changeStatus(_entry('a', untracked: true), staged: false),
              ChangeStatus.untracked,
              reason: caseName,
            );
            expect(
              changeStatus(
                _entry('b', staged: 'modified', conflicted: true),
                staged: true,
              ),
              ChangeStatus.conflicted,
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('groupChanges', () {
    test('lists partially staged entries in both groups', () {
      final groups = groupChanges([
        _entry('both', staged: 'modified', unstaged: 'modified'),
        _entry('index', staged: 'added'),
        _entry('new', untracked: true),
        _entry('conflict', conflicted: true),
      ]);
      expect(groups.unstaged.map((entry) => entry['path']), [
        'both',
        'new',
        'conflict',
      ]);
      expect(groups.staged.map((entry) => entry['path']), ['both', 'index']);
    });
  });

  group('lineStats', () {
    test('reads the requested side and sums totals', () {
      final entry = _entry('a', staged: 'modified');
      expect(lineStats(entry), (additions: 5, deletions: 2));
      expect(lineStats(entry, staged: true), (additions: 3, deletions: 0));
      expect(lineStats(entry, staged: false), (additions: 2, deletions: 2));
      expect(totalStats([entry, entry]), (additions: 10, deletions: 4));
      expect(lineStats({'path': 'b'}), (additions: 0, deletions: 0));
    });
  });

  group('discardKind', () {
    test('restores tracked files and trashes untracked ones', () {
      expect(
        discardKind(_entry('a', unstaged: 'modified')),
        DiscardKind.restore,
      );
      expect(discardKind(_entry('b', untracked: true)), DiscardKind.trash);
      expect(discardKind(_entry('c', staged: 'added')), DiscardKind.none);
    });
  });

  group('commitOptions', () {
    test('maps choices onto Node fields', () {
      expect(commitOptions(), containsPair('after', 'none'));
      final options = commitOptions(skipHooks: true, push: true);
      expect(options['skip_hooks'], isTrue);
      expect(options['after'], 'push');
      expect(options['signoff'], isFalse);
    });
  });

  group('references', () {
    test("resolves local and remote reference names", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "strip namespaces and match the current branch",
          (caseName) {
            expect(referenceLabel('heads/main'), 'main', reason: caseName);
            expect(
              referenceLabel('remotes/origin/main'),
              'origin/main',
              reason: caseName,
            );
            expect(referenceLabel('tags/v1'), 'v1', reason: caseName);
            expect(referenceLabel('stash'), 'stash', reason: caseName);
            expect(
              isCurrentReference('heads/main', 'main'),
              isTrue,
              reason: caseName,
            );
            expect(
              isCurrentReference('remotes/origin/main', 'main'),
              isFalse,
              reason: caseName,
            );
            expect(
              isCurrentReference('heads/main', null),
              isFalse,
              reason: caseName,
            );
            expect(shortHash('0123456789abcdef'), '0123456', reason: caseName);
          },
        ),
        (
          "split local and remote branches",
          (caseName) {
            final branches = splitBranches([
              {'name': 'origin/main', 'remote': true},
              {'name': 'main', 'remote': false},
            ]);
            expect(branches.local.single['name'], 'main', reason: caseName);
            expect(
              branches.remote.single['name'],
              'origin/main',
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
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

  group('paths', () {
    test("resolves path tails and breadcrumb ancestors", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "split names and keep short directory tails",
          (caseName) {
            expect(splitPath('a/b/c.dart'), (
              name: 'c.dart',
              directory: 'a/b',
            ), reason: caseName);
            expect(splitPath('c.dart'), (
              name: 'c.dart',
              directory: '',
            ), reason: caseName);
            expect(
              shortDirectory('apps/mobile/lib/features'),
              'lib/features',
              reason: caseName,
            );
            expect(shortDirectory('lib'), 'lib', reason: caseName);
            expect(shortDirectory(''), '', reason: caseName);
          },
        ),
        (
          "list breadcrumb ancestors from the root",
          (caseName) {
            expect(crumbs(''), [(label: '', path: '')], reason: caseName);
            expect(crumbs('apps/mobile'), [
              (label: '', path: ''),
              (label: 'apps', path: 'apps'),
              (label: 'mobile', path: 'apps/mobile'),
            ], reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });
}
