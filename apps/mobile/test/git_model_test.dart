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
    test('maps each side to its letter', () {
      final entry = _entry('a', staged: 'added', unstaged: 'modified');
      expect(changeStatus(entry, staged: true), ChangeStatus.added);
      expect(changeStatus(entry, staged: false), ChangeStatus.modified);
      expect(
        changeStatus(_entry('b', unstaged: 'deleted'), staged: false).letter,
        'D',
      );
    });

    test('marks untracked and conflicted entries', () {
      expect(
        changeStatus(_entry('a', untracked: true), staged: false),
        ChangeStatus.untracked,
      );
      expect(
        changeStatus(
          _entry('b', staged: 'modified', conflicted: true),
          staged: true,
        ),
        ChangeStatus.conflicted,
      );
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
    test('strip namespaces and match the current branch', () {
      expect(referenceLabel('heads/main'), 'main');
      expect(referenceLabel('remotes/origin/main'), 'origin/main');
      expect(referenceLabel('tags/v1'), 'v1');
      expect(referenceLabel('stash'), 'stash');
      expect(isCurrentReference('heads/main', 'main'), isTrue);
      expect(isCurrentReference('remotes/origin/main', 'main'), isFalse);
      expect(isCurrentReference('heads/main', null), isFalse);
      expect(shortHash('0123456789abcdef'), '0123456');
    });

    test('split local and remote branches', () {
      final branches = splitBranches([
        {'name': 'origin/main', 'remote': true},
        {'name': 'main', 'remote': false},
      ]);
      expect(branches.local.single['name'], 'main');
      expect(branches.remote.single['name'], 'origin/main');
    });
  });

  group('hunkLines', () {
    test('drops file metadata and keeps hunk numbering', () {
      final rows = hunkLines(
        'diff --git a/x b/x\nindex 1..2\n--- a/x\n+++ b/x\n'
        '@@ -4,2 +4,2 @@\n-old\n+new\n same\n\\ No newline at end of file',
      );
      expect(rows.map((row) => row.kind), [
        'header',
        'removed',
        'added',
        'context',
      ]);
      expect((rows[1].before, rows[2].after), (4, 4));
    });
  });

  group('paths', () {
    test('split names and keep short directory tails', () {
      expect(splitPath('a/b/c.dart'), (name: 'c.dart', directory: 'a/b'));
      expect(splitPath('c.dart'), (name: 'c.dart', directory: ''));
      expect(shortDirectory('apps/mobile/lib/features'), 'lib/features');
      expect(shortDirectory('lib'), 'lib');
      expect(shortDirectory(''), '');
    });

    test('list breadcrumb ancestors from the root', () {
      expect(crumbs(''), [(label: '', path: '')]);
      expect(crumbs('apps/mobile'), [
        (label: '', path: ''),
        (label: 'apps', path: 'apps'),
        (label: 'mobile', path: 'apps/mobile'),
      ]);
    });
  });
}
