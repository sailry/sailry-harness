import 'dart:ui';

import 'package:intl/intl.dart';

import '../../../runtime/json.dart';
import 'presentation.dart';

/// Task list sections in display order, with their localization keys.
enum TaskGroup {
  active('tasksActive'),
  today('tasksToday'),
  yesterday('yesterday'),
  week('tasksThisWeek'),
  earlier('tasksEarlier');

  const TaskGroup(this.key);
  final String key;
}

/// Execution Node time of the session's latest run: finished, else started.
DateTime? runTime(Map<String, dynamic> session) {
  final run = object(object(session['activity'])['run']);
  final value = run['finished_ms'] ?? run['started_ms'];
  return value is num
      ? DateTime.fromMillisecondsSinceEpoch(value.toInt())
      : null;
}

/// Calendar days from [time] to [now]; future times count as today.
int _daysBefore(DateTime time, DateTime now) {
  // UTC calendar dates keep daylight-saving transitions out of the day count.
  final day = DateTime.utc(time.year, time.month, time.day);
  final today = DateTime.utc(now.year, now.month, now.day);
  final days = today.difference(day).inDays;
  return days < 0 ? 0 : days;
}

TaskGroup dayGroup(DateTime? time, DateTime now) {
  if (time == null) return TaskGroup.earlier;
  return switch (_daysBefore(time, now)) {
    0 => TaskGroup.today,
    1 => TaskGroup.yesterday,
    < 7 => TaskGroup.week,
    _ => TaskGroup.earlier,
  };
}

/// Running, queued, stopping and waiting sessions stay above dated sections.
TaskGroup sessionGroup(Map<String, dynamic> session, DateTime now) =>
    switch (status(session)) {
      'running' || 'waiting' => TaskGroup.active,
      _ => dayGroup(runTime(session), now),
    };

/// Groups [items] by [group], keeping the incoming order inside each section.
List<(TaskGroup, List<T>)> groupTasks<T>(
  Iterable<T> items,
  TaskGroup Function(T item) group,
) {
  final sections = {for (final value in TaskGroup.values) value: <T>[]};
  for (final item in items) {
    sections[group(item)]!.add(item);
  }
  return [
    for (final value in TaskGroup.values)
      if (sections[value]!.isNotEmpty) (value, sections[value]!),
  ];
}

/// Today as a clock time, yesterday by name, the last week by weekday,
/// otherwise a short date that includes the year only when it differs.
String timeLabel(
  DateTime time,
  DateTime now, {
  required String locale,
  required String yesterday,
}) {
  final days = _daysBefore(time, now);
  if (days == 0) return DateFormat('HH:mm', locale).format(time);
  if (days == 1) return yesterday;
  if (days < 7) return DateFormat.E(locale).format(time);
  return DateFormat(
    time.year == now.year ? 'M/d' : 'y/M/d',
    locale,
  ).format(time);
}

/// Maps an application locale to intl's date symbol locale.
String dateLocale(Locale locale) {
  final tag = locale.scriptCode == 'Hant' ? 'zh_TW' : locale.toString();
  return DateFormat.localeExists(tag) ? tag : locale.languageCode;
}
