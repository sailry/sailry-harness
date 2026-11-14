import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:sailry_mobile/features/conversations/live/presentation.dart';
import 'package:sailry_mobile/features/conversations/live/recency.dart';
import 'package:sailry_mobile/ui/theme.dart';

Map<String, dynamic> _session(
  String? status, {
  int? started,
  int? finished,
  String? waiting,
  bool unread = false,
}) => {
  'activity': {
    'run': status == null
        ? null
        : {'status': status, 'started_ms': started, 'finished_ms': finished},
    'waiting': waiting,
    'attention': {'unread': unread},
  },
};

void main() {
  setUpAll(() => initializeDateFormatting());
  // Wednesday afternoon, local time.
  final now = DateTime(2026, 10, 28, 15, 30);
  String label(DateTime time, [String locale = 'en']) =>
      timeLabel(time, now, locale: locale, yesterday: 'Yesterday');

  test('ordinary task lists and counts exclude scoped assistants', () {
    for (final project in [null, 'project']) {
      for (final archived in [false, true]) {
        final session = <String, dynamic>{
          'project': project,
          'archived': archived,
          'config': {'assistant': null, 'resource': null},
        };
        expect(ordinarySession(session), isTrue);
        for (final config in [
          {
            'assistant': {'plugin': 'databases', 'id': 'database'},
          },
          {
            'resource': {'kind': 'database', 'id': 'resource'},
          },
        ]) {
          expect(ordinarySession({...session, 'config': config}), isFalse);
        }
        expect(
          ordinarySession({
            ...session,
            'delegation': {'session': 'parent'},
          }),
          isFalse,
        );
      }
    }
  });

  group('run time', () {
    test("prefers terminal timestamps and preserves missing times", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "prefers finish over start",
          (caseName) {
            final time = runTime(
              _session('completed', started: 1000, finished: 9000),
            );
            expect(
              time,
              DateTime.fromMillisecondsSinceEpoch(9000),
              reason: caseName,
            );
          },
        ),
        (
          "falls back to start while running",
          (caseName) {
            final time = runTime(_session('running', started: 1000));
            expect(
              time,
              DateTime.fromMillisecondsSinceEpoch(1000),
              reason: caseName,
            );
          },
        ),
        (
          "is absent without a run timestamp",
          (caseName) {
            expect(runTime(_session(null)), isNull, reason: caseName);
            expect(runTime(_session('queued')), isNull, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('time label', () {
    test("labels calendar distance and clock skew", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (
          label(DateTime(2026, 10, 28, 9, 5)),
          '09:05',
          "shows clock time today",
        ),
        (label(DateTime(2026, 10, 27, 23, 59)), 'Yesterday', "names yesterday"),
        (
          label(DateTime(2026, 10, 26, 8)),
          'Mon',
          "shows the weekday within a week",
        ),
        (
          label(DateTime(2026, 10, 22, 8)),
          'Thu',
          "shows the weekday within a week",
        ),
        (
          label(DateTime(2026, 10, 26, 8), 'zh'),
          '周一',
          "shows the weekday within a week",
        ),
        (
          label(DateTime(2026, 10, 21, 8)),
          '10/21',
          "shows a short date after a week",
        ),
        (
          label(DateTime(2025, 10, 24, 8)),
          '2025/10/24',
          "adds the year across years",
        ),
        (
          label(DateTime(2026, 10, 29, 1, 2)),
          '01:02',
          "treats future clock skew as today",
        ),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });

  group('sections', () {
    test("groups active sessions and calendar days in order", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "split by calendar day",
          (caseName) {
            expect(
              dayGroup(DateTime(2026, 10, 28, 0, 1), now),
              TaskGroup.today,
              reason: caseName,
            );
            expect(
              dayGroup(DateTime(2026, 10, 27, 12), now),
              TaskGroup.yesterday,
              reason: caseName,
            );
            expect(
              dayGroup(DateTime(2026, 10, 22, 12), now),
              TaskGroup.week,
              reason: caseName,
            );
            expect(
              dayGroup(DateTime(2026, 10, 21, 12), now),
              TaskGroup.earlier,
              reason: caseName,
            );
            expect(dayGroup(null, now), TaskGroup.earlier, reason: caseName);
          },
        ),
        (
          "keep active sessions first",
          (caseName) {
            final today = DateTime(2026, 10, 28, 9).millisecondsSinceEpoch;
            for (final session in [
              _session('running', started: today),
              _session('queued'),
              _session('stopping', started: today),
              _session('completed', finished: today, waiting: 'approval'),
            ]) {
              expect(
                sessionGroup(session, now),
                TaskGroup.active,
                reason: caseName,
              );
            }
            expect(
              sessionGroup(_session('failed', finished: today), now),
              TaskGroup.today,
              reason: caseName,
            );
          },
        ),
        (
          "preserve order inside each section",
          (caseName) {
            final groups = groupTasks(
              ['a1', 'b1', 'a2', 'c1', 'b2'],
              (item) => switch (item[0]) {
                'a' => TaskGroup.earlier,
                'b' => TaskGroup.active,
                _ => TaskGroup.today,
              },
            );
            expect(groups.map((section) => section.$1), [
              TaskGroup.active,
              TaskGroup.today,
              TaskGroup.earlier,
            ], reason: caseName);
            expect(groups.map((section) => section.$2), [
              ['b1', 'b2'],
              ['c1'],
              ['a1', 'a2'],
            ], reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('status dot', () {
    test("derives active attention and terminal tones", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (
          listTone(_session('running')),
          StatusTone.running,
          "follows run state",
        ),
        (listTone(_session('queued')), StatusTone.running, "follows run state"),
        (
          listTone(_session('stopping')),
          StatusTone.running,
          "follows run state",
        ),
        (
          listTone(_session('running', waiting: 'input')),
          StatusTone.warning,
          "follows run state",
        ),
        (listTone(_session('failed')), StatusTone.danger, "follows run state"),
        (
          listTone(_session('interrupted')),
          StatusTone.danger,
          "follows run state",
        ),
        (listTone(_session(null)), isNull, "follows run state"),
        (listTone(_session('cancelled')), isNull, "follows run state"),
        (
          listTone(_session('completed', unread: true)),
          StatusTone.success,
          "shows completion only while unread",
        ),
        (
          listTone(_session('completed')),
          isNull,
          "shows completion only while unread",
        ),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });

  test('maps traditional Chinese to a dated locale', () {
    expect(
      dateLocale(
        const Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
      ),
      'zh_TW',
    );
    expect(dateLocale(const Locale('pt', 'BR')), 'pt_BR');
  });
}
