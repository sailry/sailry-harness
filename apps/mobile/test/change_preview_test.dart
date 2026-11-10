import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/diff.dart';
import 'package:sailry_mobile/features/conversations/live/change_preview.dart';
import 'package:sailry_mobile/l10n/generated/app_localizations.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  test('titles distinguish one file from many', () {
    final strings = lookupAppLocalizations(const Locale('en'));
    expect(changeTitle(strings.tr, 1), 'Apply patch to 1 file');
    expect(changeTitle(strings.tr, 3), 'Apply patch to 3 files');
  });

  test('Node preview diffs keep hunks without file headers', () {
    final rows = hunkLines(
      'diff --git a/src/edit.rs b/src/edit.rs\n'
      'index 5626abf..f719efd 100644\n'
      '--- a/src/edit.rs\n'
      '+++ b/src/edit.rs\n'
      '@@ -1,2 +1,2 @@\n'
      '-old\n'
      '+new\n'
      ' same\n',
    );
    expect(
      [for (final row in rows) (row.kind, row.before, row.after)],
      [
        ('header', null, null),
        ('removed', 1, null),
        ('added', null, 1),
        ('context', 2, 2),
      ],
    );
  });
}
