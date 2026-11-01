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

  group('run time', () {
    test('prefers finish over start', () {
      final time = runTime(
        _session('completed', started: 1000, finished: 9000),
      );
      expect(time, DateTime.fromMillisecondsSinceEpoch(9000));
    });

    test('falls back to start while running', () {
      final time = runTime(_session('running', started: 1000));
      expect(time, DateTime.fromMillisecondsSinceEpoch(1000));
    });

    test('is absent without a run timestamp', () {
      expect(runTime(_session(null)), isNull);
      expect(runTime(_session('queued')), isNull);
    });
  });

  group('time label', () {
    test('shows clock time today', () {
      expect(label(DateTime(2026, 10, 28, 9, 5)), '09:05');
    });

    test('names yesterday', () {
      expect(label(DateTime(2026, 10, 27, 23, 59)), 'Yesterday');
    });

    test('shows the weekday within a week', () {
      expect(label(DateTime(2026, 10, 26, 8)), 'Mon');
      expect(label(DateTime(2026, 10, 22, 8)), 'Thu');
      expect(label(DateTime(2026, 10, 26, 8), 'zh'), '周一');
    });

    test('shows a short date after a week', () {
      expect(label(DateTime(2026, 10, 21, 8)), '10/21');
    });

    test('adds the year across years', () {
      expect(label(DateTime(2025, 10, 24, 8)), '2025/10/24');
    });

    test('treats future clock skew as today', () {
      expect(label(DateTime(2026, 10, 29, 1, 2)), '01:02');
    });
  });

  group('sections', () {
    test('split by calendar day', () {
      expect(dayGroup(DateTime(2026, 10, 28, 0, 1), now), TaskGroup.today);
      expect(dayGroup(DateTime(2026, 10, 27, 12), now), TaskGroup.yesterday);
      expect(dayGroup(DateTime(2026, 10, 22, 12), now), TaskGroup.week);
      expect(dayGroup(DateTime(2026, 10, 21, 12), now), TaskGroup.earlier);
      expect(dayGroup(null, now), TaskGroup.earlier);
    });

    test('keep active sessions first', () {
      final today = DateTime(2026, 10, 28, 9).millisecondsSinceEpoch;
      for (final session in [
        _session('running', started: today),
        _session('queued'),
        _session('stopping', started: today),
        _session('completed', finished: today, waiting: 'approval'),
      ]) {
        expect(sessionGroup(session, now), TaskGroup.active);
      }
      expect(
        sessionGroup(_session('failed', finished: today), now),
        TaskGroup.today,
      );
    });

    test('preserve order inside each section', () {
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
      ]);
      expect(groups.map((section) => section.$2), [
        ['b1', 'b2'],
        ['c1'],
        ['a1', 'a2'],
      ]);
    });
  });

  group('status dot', () {
    test('follows run state', () {
      expect(listTone(_session('running')), StatusTone.running);
      expect(listTone(_session('queued')), StatusTone.running);
      expect(listTone(_session('stopping')), StatusTone.running);
      expect(
        listTone(_session('running', waiting: 'input')),
        StatusTone.warning,
      );
      expect(listTone(_session('failed')), StatusTone.danger);
      expect(listTone(_session('interrupted')), StatusTone.danger);
      expect(listTone(_session(null)), isNull);
      expect(listTone(_session('cancelled')), isNull);
    });

    test('shows completion only while unread', () {
      expect(listTone(_session('completed', unread: true)), StatusTone.success);
      expect(listTone(_session('completed')), isNull);
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
